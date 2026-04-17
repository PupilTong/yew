//! Ported from tests-archive/integration/use_state.rs
//! :: deref_remains_valid_across_multiple_dispatches_in_callback
//!
//! Verifies that UseStateHandle::deref() keeps the dereferenced value alive
//! across subsequent dispatches within the same callback (the deref_history fix).

use std::cell::RefCell;
use std::rc::Rc;

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

thread_local! {
    static DEREF_RESULT: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[function_component]
fn UBTestComponent() -> Html {
    let state = use_state(|| "initial".to_string());

    let trigger = {
        let state = state.clone();
        Callback::from(move |_: ()| {
            // Dispatch 1: RefCell now holds Rc("first_dispatch", refcount=1)
            state.set("first_dispatch".to_string());

            // Deref: with deref_history, the Rc is cloned into a Vec (refcount=2).
            // Without the fix refcount stays 1 and the next dispatch would free it.
            // The explicit &*state is load-bearing — it's exactly the deref call
            // path this test exists to exercise; auto-deref would bypass it.
            #[allow(clippy::explicit_auto_deref)]
            let borrowed: &String = &*state;

            // Dispatch 2: RefCell updated to "second_dispatch". Old Rc refcount
            // drops to 1 (still alive via deref_history Vec), so `borrowed` is valid.
            state.set("second_dispatch".to_string());

            // Churn the allocator to maximise the chance of catching a use-after-free.
            for _ in 0..256 {
                let overwrite = Box::new([0xFFu8; 32]);
                std::hint::black_box(&*overwrite);
                drop(overwrite);
            }
            let _noise: Vec<String> = (0..64).map(|i| format!("noise_{:032}", i)).collect();

            // Read through `borrowed` — must still be "first_dispatch".
            let value = borrowed.clone();
            DEREF_RESULT.with(|r| {
                *r.borrow_mut() = Some(value);
            });
        })
    };

    html! {
        <div>
            <button onclick={trigger}>{"Trigger"}</button>
            <div>{ (*state).clone() }</div>
        </div>
    }
}

#[no_mangle]
pub extern "C" fn run() -> i32 {
    DEREF_RESULT.with(|r| *r.borrow_mut() = None);
    rust_wasm_binding::reset_scratch();

    let root = Rc::new(Element::new("div").expect("create root"));
    let root_id = root.id();
    rust_wasm_binding::append_element(0, root_id).expect("append root");

    let _app = yew::Renderer::<UBTestComponent>::with_root(root).render();

    // Traverse: root → component div → button (first child)
    let comp_div = rust_wasm_binding::get_first_child(root_id)
        .expect("component output div");
    let button_id = rust_wasm_binding::get_first_child(comp_div)
        .expect("trigger button");

    rust_wasm_binding::dispatch_event(button_id, "click", true, true, false)
        .expect("dispatch click");

    // Flush re-renders scheduled by the two set() calls in the callback.
    yew::scheduler::flush();

    let captured = DEREF_RESULT.with(|r| r.borrow().clone());
    assert_eq!(
        captured,
        Some("first_dispatch".to_string()),
        "deref() must remain valid across subsequent dispatches (got {captured:?})"
    );
    0
}
