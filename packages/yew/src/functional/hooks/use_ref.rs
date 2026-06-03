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
/// ```rust,ignore
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
///             message.set("Message limit reached".to_string());
///         } else {
///             message_count.set(message_count.get() + 1);
///             message.set("Message sent".to_string());
///         }
///     });
///
///     let onchange = {
///         let message = message.clone();
///         Callback::from(move |_| message.set("changed".to_string()))
///     };
///
///     html! {
///         <div>
///             <input {onchange} value={(*message).clone()} />
///             <button {onclick}>{ "Send" }</button>
///         </div>
///     }
/// }
/// ```
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
/// ```rust,ignore
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
///             message.set("Message limit reached".to_string());
///         } else {
///             *message_count.borrow_mut() += 1;
///             message.set("Message sent".to_string());
///         }
///     });
///
///     let onchange = {
///         let message = message.clone();
///         Callback::from(move |_| message.set("changed".to_string()))
///     };
///
///     html! {
///         <div>
///             <input {onchange} value={(*message).clone()} />
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
/// ```rust,ignore
/// use yew::{component, html, use_effect_with, use_node_ref, Html};
///
/// #[component(UseNodeRef)]
/// pub fn node_ref_hook() -> Html {
///     let div_ref = use_node_ref();
///
///     {
///         let div_ref = div_ref.clone();
///
///         use_effect_with(div_ref, |div_ref| {
///             let attached = div_ref.get().is_some();
///             move || drop(attached)
///         });
///     }
///
///     html! {
///         <div ref={div_ref}>
///             { "Click me and watch the console log!" }
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
