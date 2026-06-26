// Copyright 2026 The Lynx Authors. All rights reserved.
// Licensed under the Apache License Version 2.0 that can be found in the
// LICENSE file in the root directory of this source tree.

use std::mem;
use std::rc::Rc;

use lynx_sys::{raw, Element};
use yew::prelude::*;

const LEVEL1: [&str; 3] = ["77", "00", "ff"];
const LEVEL2: [&str; 16] = [
    "00", "11", "22", "33", "44", "55", "66", "77", "88", "99", "aa", "bb", "cc", "dd", "ee", "ff",
];
const LEVEL3: [&str; 16] = LEVEL2;
const LEVEL4: [&str; 8] = ["00", "11", "22", "33", "44", "55", "66", "77"];

const APP_STYLES: yew::css::CSSTokenStream = yew::CSS!(
    r#"
.root {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
}

.outer {
  margin: 1px;
  height: 100%;
  display: flex;
  flex-direction: row;
}

.block1 {
  margin: 1px;
  width: 100%;
  display: flex;
  flex-direction: column;
}

.block2 {
  margin: 1px;
  height: 100%;
  display: flex;
  flex-wrap: wrap;
  flex-direction: row;
  justify-content: center;
  align-content: center;
  align-items: center;
}

.block3 {
  width: 15%;
  height: 20%;
  margin: 1px;
}
"#
);

fn color_style(red: &str, green: &str, blue: &str) -> String {
    format!("background-color: #{red}{green}{blue};")
}

#[function_component(App)]
fn app() -> Html {
    html! {
        <view class="root">
            { for LEVEL1.iter().map(|color1| {
                html! {
                    <view class="outer" style={color_style(color1, color1, color1)}>
                        { for LEVEL2.iter().map(move |color2| {
                            html! {
                                <view class="block1" style={color_style(color1, color2, color2)}>
                                    { for LEVEL3.iter().map(move |color3| {
                                        html! {
                                            <view class="block2" style={color_style(color1, color2, color3)}>
                                                { for LEVEL4.iter().map(move |color4| {
                                                    html! {
                                                        <view
                                                            class="block3"
                                                            style={color_style(color2, color3, color4)}
                                                        />
                                                    }
                                                }) }
                                            </view>
                                        }
                                    }) }
                                </view>
                            }
                        }) }
                    </view>
                }
            }) }
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
