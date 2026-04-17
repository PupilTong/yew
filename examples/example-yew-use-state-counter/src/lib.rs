//! Ported from tests-archive/integration/use_state.rs :: use_state_works
//!
//! Mounts a component that increments a counter until it reaches 5.
//! Asserts the counter stabilises at 5 before run() returns.

use std::rc::Rc;
use std::sync::atomic::{AtomicI32, Ordering};

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

static FINAL_COUNT: AtomicI32 = AtomicI32::new(-1);

#[function_component]
fn UseComponent() -> Html {
    let counter = use_state(|| 0i32);
    if *counter < 5 {
        counter.set(*counter + 1);
    }
    FINAL_COUNT.store(*counter, Ordering::Relaxed);
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
    FINAL_COUNT.store(-1, Ordering::Relaxed);
    rust_wasm_binding::reset_scratch();

    let root = Rc::new(Element::new("div").expect("create root"));
    rust_wasm_binding::append_element(0, root.id()).expect("append root");

    // render() drives the scheduler synchronously; all re-renders from
    // counter.set() complete before render() returns.
    let _app = yew::Renderer::<UseComponent>::with_root(root).render();

    let final_count = FINAL_COUNT.load(Ordering::Relaxed);
    assert_eq!(final_count, 5, "counter must stabilise at 5, got {final_count}");
    0
}
