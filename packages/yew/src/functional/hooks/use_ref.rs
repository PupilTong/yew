use std::cell::RefCell;
use std::rc::Rc;

use crate::functional::{hook, use_state, Hook, HookContext};
use crate::NodeRef;

struct UseRef<F> {
    init_fn: F,
}

impl<T: 'static, F: FnOnce() -> T> Hook for UseRef<F> {
    type Output = Rc<T>;

    fn run(self, ctx: &mut HookContext) -> Self::Output {
        ctx.next_state(|_| (self.init_fn)())
    }
}

/// This hook is used for obtaining a reference to a stateful value.
/// Its state persists across renders.
///
/// Mutation must be done via interior mutability, such as `Cell` or `RefCell`.
///
/// It is important to note that you do not get notified of state changes.
/// If you need the component to be re-rendered on state change, consider using
/// [`use_state`](super::use_state()).
///
/// # Example
/// ```rust
/// use std::cell::Cell;
/// use std::ops::{Deref, DerefMut};
/// use std::rc::Rc;
///
/// use yew::prelude::*;
///
/// #[component(UseRef)]
/// fn ref_hook() -> Html {
///     let message = use_state(|| "".to_string());
///     let message_count = use_ref(|| Cell::new(0));
///
///     let onclick = Callback::from(move |_| {
///         if message_count.get() > 3 {
///             tracing::warn!("Message limit reached");
///         } else {
///             message_count.set(message_count.get() + 1);
///             tracing::info!("Message sent");
///         }
///     });
///
///     html! {
///         <div>
///             <input value={(*message).clone()} />
///             <button {onclick}>{ "Send" }</button>
///         </div>
///     }
/// }
pub fn use_ref<T: 'static, F>(init_fn: F) -> impl Hook<Output = Rc<T>>
where
    F: FnOnce() -> T,
{
    UseRef { init_fn }
}

/// This hook is used for obtaining a mutable reference to a stateful value.
/// Its state persists across renders.
///
/// It is important to note that you do not get notified of state changes.
/// If you need the component to be re-rendered on state change, consider using
/// [`use_state`](super::use_state()).
///
/// # Example
/// ```rust
/// use std::cell::RefCell;
/// use std::ops::{Deref, DerefMut};
/// use std::rc::Rc;
///
/// use yew::prelude::*;
///
/// #[component(UseRef)]
/// fn ref_hook() -> Html {
///     let message = use_state(|| "".to_string());
///     let message_count = use_mut_ref(|| 0);
///
///     let onclick = Callback::from(move |_| {
///         if *message_count.borrow_mut() > 3 {
///             tracing::warn!("Message limit reached");
///         } else {
///             *message_count.borrow_mut() += 1;
///             tracing::info!("Message sent");
///         }
///     });
///
///     html! {
///         <div>
///             <input value={(*message).clone()} />
///             <button {onclick}>{ "Send" }</button>
///         </div>
///     }
/// }
/// ```
pub fn use_mut_ref<T: 'static, F>(init_fn: F) -> impl Hook<Output = Rc<RefCell<T>>>
where
    F: FnOnce() -> T,
{
    UseRef {
        init_fn: || RefCell::new(init_fn()),
    }
}

/// This hook is used for obtaining a [`NodeRef`].
/// It persists across renders.
///
/// The `ref` attribute can be used to attach the [`NodeRef`] to an HTML element. In callbacks,
/// you can then get the DOM `Element` that the `ref` is attached to.
///
/// # Example
///
/// ```rust
/// use yew::{component, html, use_effect_with, use_node_ref, Html, Callback};
///
/// #[component(UseNodeRef)]
/// pub fn node_ref_hook() -> Html {
///     let div_ref = use_node_ref();
///
///     // In Paws, event listeners are attached declaratively via the html! macro
///     // rather than through imperative DOM APIs. Use `onclick`, `oninput`, etc.
///     // attributes directly on elements. The NodeRef is still useful for reading
///     // element state after render via use_effect_with.
///
///     let onclick = Callback::from(|_| {
///         tracing::info!("Clicked!");
///     });
///
///     html! {
///         <div ref={div_ref} {onclick}>
///             { "Click me and watch the log!" }
///         </div>
///     }
/// }
/// ```
///
/// # Tip
///
/// When conditionally rendering elements you can use `NodeRef` in conjunction with
/// `use_effect_with` to perform actions each time an element is rendered and just before the
/// component where the hook is used in is going to be removed from the DOM.
#[hook]
pub fn use_node_ref() -> NodeRef {
    (*use_state(NodeRef::default)).clone()
}
