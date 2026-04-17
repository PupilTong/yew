//! Ported from tests-archive/integration/use_state.rs :: use_state_eq_works
//!
//! use_state_eq skips re-renders when the new value equals the current value.
//! The component always calls counter.set(1):
//!   - Render 1: counter=0, set(1) → 1 != 0 → re-render scheduled
//!   - Render 2: counter=1, set(1) → 1 == 1 → no re-render
//!
//! Expected RENDER_COUNT: 2

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

static RENDER_COUNT: AtomicUsize = AtomicUsize::new(0);

#[function_component]
fn UseComponent() -> Html {
    RENDER_COUNT.fetch_add(1, Ordering::Relaxed);
    let counter = use_state_eq(|| 0i32);
    counter.set(1);
    html! {
        <div>
            {"Test Output: "}
            <div id="result">{ *counter }</div>
            {"\n"}
        </div>
    }
}

#[no_mangle]
pub extern "C" fn run() -> i32 {
    RENDER_COUNT.store(0, Ordering::Relaxed);
    rust_wasm_binding::reset_scratch();

    let root = Rc::new(Element::new("div").expect("create root"));
    rust_wasm_binding::append_element(0, root.id()).expect("append root");

    let _app = yew::Renderer::<UseComponent>::with_root(root).render();

    let count = RENDER_COUNT.load(Ordering::Relaxed);
    assert_eq!(count, 2, "use_state_eq must render exactly twice, got {count}");
    0
}
