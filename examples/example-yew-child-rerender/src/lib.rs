//! Ported from tests-archive/integration/use_state.rs
//! :: use_state_handle_as_prop_triggers_child_rerender_issue_4058
//!
//! Verifies that passing a UseStateHandle as a prop to a child component
//! correctly triggers child re-renders when the state changes.

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

static CHILD_RENDER_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Properties, PartialEq)]
struct ChildProps {
    handle: UseStateHandle<i32>,
}

#[function_component]
fn ChildComponent(props: &ChildProps) -> Html {
    CHILD_RENDER_COUNT.fetch_add(1, Ordering::Relaxed);
    let onclick = {
        let handle = props.handle.clone();
        Callback::from(move |_: ()| {
            handle.set(*handle + 1);
        })
    };
    html! {
        <div>
            <button {onclick}>{"Increment"}</button>
            <div>{ *props.handle }</div>
        </div>
    }
}

#[function_component]
fn ParentComponent() -> Html {
    let state = use_state(|| 0i32);
    html! {
        <ChildComponent handle={state} />
    }
}

#[no_mangle]
pub extern "C" fn run() -> i32 {
    CHILD_RENDER_COUNT.store(0, Ordering::Relaxed);
    rust_wasm_binding::reset_scratch();

    let root = Rc::new(Element::new("div").expect("create root"));
    let root_id = root.id();
    rust_wasm_binding::append_element(0, root_id).expect("append root");

    let _app = yew::Renderer::<ParentComponent>::with_root(root).render();

    assert_eq!(
        CHILD_RENDER_COUNT.load(Ordering::Relaxed),
        1,
        "child should render once on mount"
    );

    // Traverse: root → ChildComponent output div → button (first child)
    let comp_div = rust_wasm_binding::get_first_child(root_id)
        .expect("component output div");
    let btn_increment = rust_wasm_binding::get_first_child(comp_div)
        .expect("increment button");

    rust_wasm_binding::dispatch_event(btn_increment, "click", true, true, false)
        .expect("dispatch click");

    // Flush re-renders: the state change in ChildComponent must propagate and
    // trigger a re-render of the child (issue #4058).
    yew::scheduler::flush();

    let count = CHILD_RENDER_COUNT.load(Ordering::Relaxed);
    assert!(
        count >= 2,
        "child must re-render after UseStateHandle prop changes (got {count})"
    );
    0
}
