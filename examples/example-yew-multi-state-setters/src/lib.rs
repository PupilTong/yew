//! Ported from tests-archive/integration/use_state.rs :: multiple_use_state_setters
//!
//! The component has two update paths:
//!   - An inline set (+10) that runs when counter < 11
//!   - A use_effect_with(()) cleanup that runs once after mount (+1)
//!
//! Expected final value: 11  (0 → +10 inline → effect +1 → 11, stable).

use std::rc::Rc;
use std::sync::atomic::{AtomicI32, Ordering};

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

static FINAL_COUNT: AtomicI32 = AtomicI32::new(-1);

#[function_component]
fn UseComponent() -> Html {
    let counter = use_state(|| 0i32);
    let counter_clone = counter.clone();
    use_effect_with((), move |_| {
        counter_clone.set(*counter_clone + 1);
        || {}
    });
    let another_scope = {
        let counter = counter.clone();
        move || {
            if *counter < 11 {
                counter.set(*counter + 10);
            }
        }
    };
    another_scope();
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

    // render() drives the scheduler synchronously to completion.
    // Sequence: first render (counter=0, inline sets 10) → effect fires (+1 → 11)
    // → second render (11, no inline set) → stable at 11.
    let _app = yew::Renderer::<UseComponent>::with_root(root).render();

    let final_count = FINAL_COUNT.load(Ordering::Relaxed);
    assert_eq!(final_count, 11, "counter must reach 11, got {final_count}");
    0
}
