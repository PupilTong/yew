use std::cell::Cell;
use std::mem;
use std::rc::Rc;

use lynx_sys::{raw, Element, TimerId};
use yew::prelude::*;

const ARROW: &str = inline_image!("assets/arrow.png");
const LYNX_LOGO: &str = inline_image!("assets/lynx-logo.png");
const YEW_LOGO: &str = inline_image!("assets/yew-logo.png");

const APP_STYLES: yew::css::CSSTokenStream = yew::CSS!(
    r#"
.Page {
  min-height: 100vh;
  background-color: #000;
  position: relative;
  display: flex;
  flex-direction: column;
}

.Background {
  position: fixed;
  background: radial-gradient(71.43% 62.3% at 46.43% 36.43%, rgba(18, 229, 229, 0) 15%, rgba(239, 155, 255, 0.3) 56.35%, #ff6448 100%);
  box-shadow: 0px 12.93px 28.74px 0px #ffd28db2 inset;
  border-radius: 50%;
  width: 200vw;
  height: 200vw;
  top: -60vw;
  left: -14.27vw;
  transform: rotate(15.25deg);
}

.App {
  position: relative;
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
}

text {
  color: #fff;
}

.Banner {
  flex: 5;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.Logo {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  margin-bottom: 8px;
}

.Logo--yew {
  width: 100px;
  height: 100px;
  animation: Logo--spin infinite 20s linear;
}

.Logo--lynx {
  width: 100px;
  height: 100px;
  animation: Logo--shake infinite 0.5s ease;
}

@keyframes Logo--spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

@keyframes Logo--shake {
  0% {
    transform: scale(1);
  }
  50% {
    transform: scale(0.9);
  }
  100% {
    transform: scale(1);
  }
}

.Content {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
}

.Arrow {
  width: 24px;
  height: 24px;
}

.Title {
  font-size: 36px;
  font-weight: 700;
}

.Subtitle {
  font-style: italic;
  font-size: 22px;
  font-weight: 600;
  margin-bottom: 8px;
}

.Description {
  font-size: 20px;
  color: rgba(255, 255, 255, 0.85);
  margin: 15rpx;
}

.Hint {
  font-size: 12px;
  margin: 5px;
  color: rgba(255, 255, 255, 0.65);
}
"#
);

const HINT_EMPHASIS_STYLE: &str = "font-style: italic; color: rgba(255, 255, 255, 0.85);";
const FILLER_STYLE: &str = "flex: 1;";
const NO_TIMER: TimerId = -1;
const FLAP_GRAVITY: f64 = 0.6;
const FLAP_JUMP_FORCE: f64 = -12.0;
const FLAP_STACK_FACTOR: f64 = 0.6;
const FLAP_FRAME_MS: i64 = 16;

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

    const fn class_name(self) -> &'static str {
        match self {
            Self::Lynx => "Logo--lynx",
            Self::Yew => "Logo--yew",
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

fn logo_style(y: f64) -> String {
    format!("transform: translateY({y:.2}px);")
}

struct FlappyEngine {
    y: Cell<f64>,
    velocity: Cell<f64>,
    timer: Cell<TimerId>,
    on_update: Callback<f64>,
}

impl FlappyEngine {
    fn new(on_update: Callback<f64>) -> Self {
        Self {
            y: Cell::new(0.0),
            velocity: Cell::new(0.0),
            timer: Cell::new(NO_TIMER),
            on_update,
        }
    }

    fn jump(self: &Rc<Self>) {
        let velocity =
            (self.velocity.get() + FLAP_JUMP_FORCE * FLAP_STACK_FACTOR).max(FLAP_JUMP_FORCE);
        self.velocity.set(velocity);
        if self.timer.get() == NO_TIMER {
            self.schedule_tick();
        }
    }

    fn schedule_tick(self: &Rc<Self>) {
        let engine = self.clone();
        let timer = lynx_sys::set_timeout(move || engine.tick(), FLAP_FRAME_MS);
        self.timer.set(timer);
    }

    fn tick(self: Rc<Self>) {
        let velocity = self.velocity.get() + FLAP_GRAVITY;
        let y = self.y.get() + velocity;
        if y >= 0.0 {
            self.y.set(0.0);
            self.velocity.set(0.0);
            self.timer.set(NO_TIMER);
            self.on_update.emit(0.0);
            return;
        }

        self.y.set(y);
        self.velocity.set(velocity);
        self.on_update.emit(y);
        self.schedule_tick();
    }
}

#[function_component(App)]
fn app() -> Html {
    let logo = use_state(Logo::default);
    let logo_y = use_state(|| 0.0);
    let flappy = {
        let logo_y = logo_y.clone();
        use_memo((), move |_| {
            FlappyEngine::new(Callback::from(move |y| logo_y.set(y)))
        })
    };

    let jump = {
        let flappy = flappy.clone();
        Callback::from(move |_| flappy.jump())
    };
    let toggle_logo = {
        let logo = logo.clone();
        Callback::from(move |_| {
            logo.set((*logo).toggled());
        })
    };
    let logo_value = *logo;
    let logo_style = logo_style(*logo_y);

    html! {
        <view class="Page" ontap={jump}>
            <view class="Background" />
            <view class="App">
                <view class="Banner">
                    <view class="Logo" style={logo_style} ontap={toggle_logo}>
                        <image
                            src={logo_value.source()}
                            class={logo_value.class_name()}
                        />
                    </view>
                    <text class="Title">{ "Yew" }</text>
                    <text class="Subtitle">{ "on Lynx" }</text>
                </view>
                <view class="Content">
                    <image src={ARROW} class="Arrow" />
                    <text class="Description">{ "Tap the logo and have fun!" }</text>
                    <text class="Hint">
                        { "Edit" }
                        <text style={HINT_EMPHASIS_STYLE}>{ " src/main.rs " }</text>
                        { "to see updates!" }
                    </text>
                </view>
                <view style={FILLER_STYLE} />
            </view>
        </view>
    }
}

fn main() {
    raw::replace_style_sheets_tokens(APP_STYLES);
    let root = page_root();
    let handle = yew::Renderer::<App>::with_root(root).render();
    mem::forget(handle);
}

fn page_root() -> Rc<Element> {
    let root = raw::get_page_element().unwrap_or_else(raw::create_page);
    Rc::new(Element::from_raw_unchecked(root))
}
