//! Mounts a minimal yew counter component on the Paws host.
//!
//! Tests the full integration: yew virtual DOM reconciliation,
//! element creation via `rust_wasm_binding`, attribute/text diffing,
//! and event listener registration through the host dispatch pipeline.

use std::rc::Rc;

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

/// A minimal counter component with a "+" button and a count display.
#[function_component]
fn Counter() -> Html {
    let count = use_state(|| 0i32);

    let onclick = {
        let count = count.clone();
        Callback::from(move |_: ()| {
            count.set(*count + 1);
        })
    };

    html! {
        <div class="counter">
            <button {onclick}>{ "+" }</button>
            <span>{ format!("{}", *count) }</span>
        </div>
    }
}

/// Entry point called by the Paws host.
///
/// Creates a root `<div>` element, appends it to the document, and
/// mounts the yew counter component into it. After `render()` returns
/// the virtual DOM has been reconciled and the physical DOM tree is
/// populated.
#[no_mangle]
pub extern "C" fn run() -> i32 {
    rust_wasm_binding::reset_scratch();

    let root = match Element::new("div") {
        Ok(element) => Rc::new(element),
        Err(error_code) => return error_code,
    };

    if let Err(error_code) = rust_wasm_binding::append_element(0, root.id()) {
        return error_code;
    }

    let _app = yew::Renderer::<Counter>::with_root(root).render();

    0
}
