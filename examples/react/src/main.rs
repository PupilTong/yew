use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::{fmt::Write as _, mem};

use rust_wasm_binding::{raw, Element, TimerId};
use yew::prelude::*;

const ARROW: &str = inline_image!("assets/arrow.png");
const LYNX_LOGO: &str = inline_image!("assets/lynx-logo.png");
const YEW_LOGO: &str = inline_image!("assets/yew-logo.png");

const GRAVITY: f64 = 0.6;
const JUMP_FORCE: f64 = -12.0;
const STACK_FACTOR: f64 = 0.6;
const FRAME_MS: u64 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Logo {
    Lynx,
    Yew,
}

impl Logo {
    const fn source(self) -> &'static str {
        match self {
            Self::Lynx => LYNX_LOGO,
            Self::Yew => YEW_LOGO,
        }
    }

    const fn style(self) -> &'static str {
        match self {
            Self::Lynx => style::LOGO_LYNX,
            Self::Yew => style::LOGO_YEW,
        }
    }

    const fn toggled(self) -> Self {
        match self {
            Self::Lynx => Self::Yew,
            Self::Yew => Self::Lynx,
        }
    }
}

impl Default for Logo {
    fn default() -> Self {
        Self::Lynx
    }
}

#[derive(Debug, Default)]
struct FlappyEngine {
    y: f64,
    velocity: f64,
    timer_id: Option<TimerId>,
}

impl FlappyEngine {
    fn jump(&mut self) -> Option<f64> {
        self.velocity = (self.velocity + JUMP_FORCE * STACK_FACTOR).max(JUMP_FORCE);
        if self.timer_id.is_some() {
            None
        } else {
            Some(self.tick())
        }
    }

    fn tick(&mut self) -> f64 {
        self.velocity += GRAVITY;
        self.y += self.velocity;

        if self.y >= 0.0 {
            self.y = 0.0;
            self.velocity = 0.0;
        }

        self.y
    }

    fn is_airborne(&self) -> bool {
        self.y < 0.0
    }

    fn set_timer(&mut self, timer_id: TimerId) {
        self.timer_id = Some(timer_id);
    }

    fn clear_timer(&mut self) -> Option<TimerId> {
        self.timer_id.take()
    }
}

fn schedule_flappy_frame(
    engine: Rc<RefCell<FlappyEngine>>,
    logo_y: UseStateHandle<f64>,
    alive: Rc<Cell<bool>>,
) {
    if engine.borrow().timer_id.is_some() {
        return;
    }

    let callback_engine = engine.clone();
    let callback_logo_y = logo_y.clone();
    let callback_alive = alive.clone();
    let timer_id = rust_wasm_binding::set_timeout(
        move || {
            if !callback_alive.get() {
                callback_engine.borrow_mut().clear_timer();
                return;
            }

            let (next_y, should_continue) = {
                let mut engine = callback_engine.borrow_mut();
                engine.clear_timer();
                let next_y = engine.tick();
                (next_y, engine.is_airborne())
            };
            callback_logo_y.set(next_y);

            if should_continue {
                schedule_flappy_frame(callback_engine, callback_logo_y, callback_alive);
            }
        },
        FRAME_MS as i64,
    );
    engine.borrow_mut().set_timer(timer_id);
}

fn logo_style(y: f64) -> String {
    let mut style = String::with_capacity(style::LOGO.len() + 32);
    style.push_str(style::LOGO);
    let _ = write!(style, " transform: translateY({:.2}px);", y);
    style
}

#[function_component(App)]
fn app() -> Html {
    let logo = use_state(Logo::default);
    let logo_y = use_state(|| 0.0);
    let flappy = use_mut_ref(FlappyEngine::default);
    let alive = use_mut_ref(|| Rc::new(Cell::new(true)));

    {
        let alive = alive.clone();
        let flappy = flappy.clone();
        use_effect_with((), move |_| {
            move || {
                alive.borrow().set(false);
                if let Some(timer_id) = flappy.borrow_mut().clear_timer() {
                    rust_wasm_binding::clear_timeout(timer_id);
                }
            }
        });
    }

    let play = {
        let flappy = flappy.clone();
        let logo = logo.clone();
        let logo_y = logo_y.clone();
        let alive = alive.borrow().clone();
        Callback::from(move |_| {
            logo.set((*logo).toggled());
            let immediate_y = flappy.borrow_mut().jump();
            if let Some(next_y) = immediate_y {
                logo_y.set(next_y);
                if flappy.borrow().is_airborne() {
                    schedule_flappy_frame(flappy.clone(), logo_y.clone(), alive.clone());
                }
            }
        })
    };
    let logo_value = *logo;
    let logo_style = logo_style(*logo_y);

    html! {
        <view style={style::PAGE} ontap={play}>
            <view style={style::BACKGROUND} />
            <view style={style::APP}>
                <view style={style::BANNER}>
                    <view style={logo_style}>
                        <image
                            src={logo_value.source()}
                            style={logo_value.style()}
                        />
                    </view>
                    <text style={style::TITLE}>{ "Yew" }</text>
                    <text style={style::SUBTITLE}>{ "on Lynx" }</text>
                </view>
                <view style={style::CONTENT}>
                    <image src={ARROW} style={style::ARROW} />
                    <text style={style::DESCRIPTION}>{ "Tap the logo and have fun!" }</text>
                    <text style={style::HINT}>
                        { "Edit" }
                        <text style={style::HINT_EMPHASIS}>{ " src/main.rs " }</text>
                        { "to see updates!" }
                    </text>
                </view>
                <view style={style::FILLER} />
            </view>
        </view>
    }
}

fn main() {
    let root = page_root();
    let handle = yew::Renderer::<App>::with_root(root).render();
    mem::forget(handle);
}

fn page_root() -> Rc<Element> {
    let root = raw::get_page_element();
    let root = if root.is_null() {
        raw::create_page()
    } else {
        root
    };

    Rc::new(Element::from_raw_unchecked(root))
}

mod style {
    pub const PAGE: &str = concat!(
        "min-height: 100vh;",
        "background-color: #000;",
        "position: relative;",
        "display: flex;",
        "flex-direction: column;"
    );
    pub const BACKGROUND: &str = concat!(
        "position: fixed;",
        "background: radial-gradient(71.43% 62.3% at 46.43% 36.43%, ",
        "rgba(18, 229, 229, 0) 15%, ",
        "rgba(239, 155, 255, 0.3) 56.35%, ",
        "#ff6448 100%);",
        "box-shadow: 0px 12.93px 28.74px 0px #ffd28db2 inset;",
        "border-radius: 50%;",
        "width: 200vw;",
        "height: 200vw;",
        "top: -60vw;",
        "left: -14.27vw;",
        "transform: rotate(15.25deg);"
    );
    pub const APP: &str = concat!(
        "position: relative;",
        "min-height: 100vh;",
        "display: flex;",
        "flex-direction: column;",
        "align-items: center;",
        "justify-content: center;"
    );
    pub const BANNER: &str = concat!(
        "flex: 5;",
        "display: flex;",
        "flex-direction: column;",
        "align-items: center;",
        "justify-content: center;",
        "z-index: 100;"
    );
    pub const LOGO: &str = concat!(
        "display: flex;",
        "flex-direction: column;",
        "align-items: center;",
        "justify-content: center;",
        "margin-bottom: 8px;"
    );
    pub const LOGO_YEW: &str = "width: 100px; height: 100px;";
    pub const LOGO_LYNX: &str = "width: 100px; height: 100px;";
    pub const CONTENT: &str = concat!(
        "display: flex;",
        "flex-direction: column;",
        "align-items: center;",
        "justify-content: center;"
    );
    pub const ARROW: &str = "width: 24px; height: 24px;";
    pub const TITLE: &str = "color: #fff; font-size: 36px; font-weight: 700;";
    pub const SUBTITLE: &str = concat!(
        "color: #fff;",
        "font-style: italic;",
        "font-size: 22px;",
        "font-weight: 600;",
        "margin-bottom: 8px;"
    );
    pub const DESCRIPTION: &str = concat!(
        "font-size: 20px;",
        "color: rgba(255, 255, 255, 0.85);",
        "margin: 15rpx;"
    );
    pub const HINT: &str = concat!(
        "font-size: 12px;",
        "margin: 5px;",
        "color: rgba(255, 255, 255, 0.65);"
    );
    pub const HINT_EMPHASIS: &str =
        concat!("font-style: italic;", "color: rgba(255, 255, 255, 0.85);");
    pub const FILLER: &str = "flex: 1;";
}
