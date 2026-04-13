//! Exercises yew's list reconciliation — keyed children, add/remove/reorder.
//!
//! The host calls exported functions to mutate the list state and trigger
//! yew re-renders, then inspects the DOM tree to verify correct reconciliation.

use std::cell::RefCell;
use std::rc::Rc;

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

thread_local! {
    /// Shared list state. Exported functions push/remove/reorder items
    /// and then dispatch a synthetic event to trigger a re-render.
    static LIST_STATE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Callback to force a re-render after state changes.
    static FORCE_UPDATE: RefCell<Option<Callback<()>>> = const { RefCell::new(None) };
}

/// Keyed list component that renders a `<ul>` with `<li>` children.
#[function_component]
fn KeyedList() -> Html {
    let update_trigger = use_force_update();

    // Store the force-update callback globally so exported functions can trigger re-renders.
    FORCE_UPDATE.with(|cell| {
        *cell.borrow_mut() = Some(Callback::from(move |_: ()| {
            update_trigger.force_update();
        }));
    });

    let items = LIST_STATE.with(|state| state.borrow().clone());

    html! {
        <ul>
            { for items.iter().map(|item| html! {
                <li key={item.clone()}>{ item.clone() }</li>
            }) }
        </ul>
    }
}

/// Helper: mutate the list state and force a yew re-render.
fn mutate_and_rerender(mutate: impl FnOnce(&mut Vec<String>)) {
    LIST_STATE.with(|state| mutate(&mut state.borrow_mut()));
    // Clone the callback out of the RefCell BEFORE calling emit(),
    // so the cell isn't borrowed when yew re-renders (which re-enters
    // the component's view() function and writes to FORCE_UPDATE).
    let callback = FORCE_UPDATE.with(|cell| cell.borrow().clone());
    if let Some(callback) = callback {
        callback.emit(());
    }
}

/// Mount the keyed list component. Returns 0 on success.
///
/// DOM after initial render (empty list):
///   document(0) > root(1) > ul(2)
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

    let app = yew::Renderer::<KeyedList>::with_root(root).render();
    std::mem::forget(app);
    0
}

/// Push items "A", "B", "C" and re-render.
/// DOM after: ul > li("A") + li("B") + li("C")
#[no_mangle]
pub extern "C" fn push_abc() -> i32 {
    rust_wasm_binding::reset_scratch();
    mutate_and_rerender(|items| {
        items.push("A".into());
        items.push("B".into());
        items.push("C".into());
    });
    0
}

/// Remove "B" (middle item) and re-render.
/// DOM after: ul > li("A") + li("C")
#[no_mangle]
pub extern "C" fn remove_middle() -> i32 {
    rust_wasm_binding::reset_scratch();
    mutate_and_rerender(|items| {
        items.retain(|item| item != "B");
    });
    0
}

/// Reverse the list and re-render.
/// DOM after: ul > li("C") + li("A")
#[no_mangle]
pub extern "C" fn reverse_list() -> i32 {
    rust_wasm_binding::reset_scratch();
    mutate_and_rerender(|items| {
        items.reverse();
    });
    0
}

/// Push "D" at the beginning and re-render.
/// DOM after: ul > li("D") + li("C") + li("A")
#[no_mangle]
pub extern "C" fn prepend_d() -> i32 {
    rust_wasm_binding::reset_scratch();
    mutate_and_rerender(|items| {
        items.insert(0, "D".into());
    });
    0
}

/// Returns the number of items currently in the list.
#[no_mangle]
pub extern "C" fn item_count() -> i32 {
    LIST_STATE.with(|state| state.borrow().len() as i32)
}
