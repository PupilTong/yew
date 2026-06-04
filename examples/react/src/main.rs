use std::rc::Rc;
use std::time::Duration;
use std::{fmt::Write as _, mem};

use rust_wasm_binding::{raw, Element};
use yew::platform::time::sleep;
use yew::prelude::*;

const ARROW: &str = inline_image!("assets/arrow.png");
const LYNX_LOGO: &str = inline_image!("assets/lynx-logo.png");
const REACT_LOGO: &str = inline_image!("assets/react-logo.png");

const GRAVITY: f64 = 0.6;
const JUMP_FORCE: f64 = -12.0;
const STACK_FACTOR: f64 = 0.6;
const FRAME_MS: u64 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Logo {
    Lynx,
    React,
}

impl Logo {
    const fn source(self) -> &'static str {
        match self {
            Self::Lynx => LYNX_LOGO,
            Self::React => REACT_LOGO,
        }
    }

    const fn style(self) -> &'static str {
        match self {
            Self::Lynx => style::LOGO_LYNX,
            Self::React => style::LOGO_REACT,
        }
    }

    const fn toggled(self) -> Self {
        match self {
            Self::Lynx => Self::React,
            Self::React => Self::Lynx,
        }
    }
}

impl Default for Logo {
    fn default() -> Self {
        Self::Lynx
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Flappy {
    y: f64,
    velocity: f64,
}

impl Flappy {
    fn jump(mut self) -> Self {
        self.velocity = (self.velocity + JUMP_FORCE * STACK_FACTOR).max(JUMP_FORCE);
        self.tick()
    }

    fn tick(mut self) -> Self {
        self.velocity += GRAVITY;
        self.y += self.velocity;

        if self.y >= 0.0 {
            Self::default()
        } else {
            self
        }
    }

    fn logo_style(self) -> String {
        let mut style = String::with_capacity(style::LOGO.len() + 32);
        style.push_str(style::LOGO);
        let _ = write!(style, " transform: translateY({:.2}px);", self.y);
        style
    }
}

#[derive(Debug, Default)]
struct App {
    logo: Logo,
    flappy: Flappy,
}

enum Msg {
    Jump,
    ToggleLogo,
    Tick,
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        println!("Hello, ReactLynx");
        Self::schedule_tick(ctx);
        Self::default()
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Jump => {
                self.flappy = self.flappy.jump();
                true
            }
            Msg::ToggleLogo => {
                self.logo = self.logo.toggled();
                true
            }
            Msg::Tick => {
                self.flappy = self.flappy.tick();
                Self::schedule_tick(ctx);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let jump = ctx.link().callback(|_| Msg::Jump);
        let toggle_logo = ctx.link().callback(|_| Msg::ToggleLogo);

        html! {
            <view style={style::PAGE} ontap={jump}>
                <view style={style::BACKGROUND} />
                <view style={style::APP}>
                    <view style={style::BANNER}>
                        <view style={self.flappy.logo_style()} ontap={toggle_logo}>
                            <image src={self.logo.source()} style={self.logo.style()} />
                        </view>
                        <text style={style::TITLE}>{ "React" }</text>
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
}

impl App {
    fn schedule_tick(ctx: &Context<Self>) {
        ctx.link().send_future(async {
            sleep(Duration::from_millis(FRAME_MS)).await;
            Msg::Tick
        });
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
    pub const LOGO_REACT: &str = "width: 100px; height: 100px;";
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
