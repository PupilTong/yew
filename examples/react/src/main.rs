use std::rc::Rc;
use std::{fmt::Write as _, mem};

use lynx_sys::{raw, Element};
use yew::prelude::*;

const ARROW: &str = inline_image!("assets/arrow.png");
const LYNX_LOGO: &str = inline_image!("assets/lynx-logo.png");
const YEW_LOGO: &str = inline_image!("assets/yew-logo.png");

const POP_Y: f64 = -28.0;

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

    let play = {
        let logo = logo.clone();
        let logo_y = logo_y.clone();
        Callback::from(move |_| {
            logo.set((*logo).toggled());
            logo_y.set(if *logo_y < 0.0 { 0.0 } else { POP_Y });
        })
    };
    let logo_value = *logo;
    let logo_style = logo_style(*logo_y);

    html! {
        <view style={style::PAGE} onclick={play}>
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
    let root = raw::get_page_element().unwrap_or_else(raw::create_page);
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
