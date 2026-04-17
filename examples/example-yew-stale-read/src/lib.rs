//! Ported from tests-archive/integration/use_state.rs
//! :: use_state_handles_read_latest_value_issue_3796
//!
//! Verifies that UseStateHandle always reads the latest dispatched value even
//! when a callback accesses the handle before a re-render occurs.

use std::cell::RefCell;
use std::rc::Rc;

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

thread_local! {
    static CAPTURED_VALUES: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

#[function_component]
fn FormComponent() -> Html {
    let field_a = use_state(String::new);
    let field_b = use_state(String::new);

    let update_a = {
        let field_a = field_a.clone();
        Callback::from(move |_: ()| {
            field_a.set("value_a".to_string());
        })
    };

    let update_b = {
        let field_b = field_b.clone();
        Callback::from(move |_: ()| {
            field_b.set("value_b".to_string());
        })
    };

    let submit = {
        let field_a = field_a.clone();
        let field_b = field_b.clone();
        Callback::from(move |_: ()| {
            let a = (*field_a).clone();
            let b = (*field_b).clone();
            CAPTURED_VALUES.with(|v| {
                *v.borrow_mut() = Some((a, b));
            });
        })
    };

    html! {
        <div>
            <button onclick={update_a}>{"Update A"}</button>
            <button onclick={update_b}>{"Update B"}</button>
            <button onclick={submit}>{"Submit"}</button>
            <div>{ format!("a={}, b={}", *field_a, *field_b) }</div>
        </div>
    }
}

#[no_mangle]
pub extern "C" fn run() -> i32 {
    CAPTURED_VALUES.with(|v| *v.borrow_mut() = None);
    rust_wasm_binding::reset_scratch();

    let root = Rc::new(Element::new("div").expect("create root"));
    let root_id = root.id();
    rust_wasm_binding::append_element(0, root_id).expect("append root");

    let _app = yew::Renderer::<FormComponent>::with_root(root).render();

    // Traverse: root → component div → buttons (siblings)
    let comp_div = rust_wasm_binding::get_first_child(root_id)
        .expect("component output div");
    let btn_a = rust_wasm_binding::get_first_child(comp_div)
        .expect("update-a button");
    let btn_b = rust_wasm_binding::get_next_sibling(btn_a)
        .expect("update-b button");
    let btn_submit = rust_wasm_binding::get_next_sibling(btn_b)
        .expect("submit button");

    // Click update-a and update-b WITHOUT flushing between — simulates rapid
    // input before a re-render. The submit handler must see latest values from
    // both handles despite no re-render having occurred yet.
    rust_wasm_binding::dispatch_event(btn_a, "click", true, true, false)
        .expect("dispatch update-a");
    rust_wasm_binding::dispatch_event(btn_b, "click", true, true, false)
        .expect("dispatch update-b");
    rust_wasm_binding::dispatch_event(btn_submit, "click", true, true, false)
        .expect("dispatch submit");

    // Flush any pending re-renders.
    yew::scheduler::flush();

    let captured = CAPTURED_VALUES.with(|v| v.borrow().clone());
    assert_eq!(
        captured,
        Some(("value_a".to_string(), "value_b".to_string())),
        "submit handler must see latest values for both fields (got {captured:?})"
    );
    0
}
