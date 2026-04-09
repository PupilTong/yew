#[macro_use]
mod events;

pub use events::*;

use crate::Callback;

/// A trait similar to `Into<T>` which allows conversion of a value into a [`Callback`].
/// This is used for event listeners.
pub trait IntoEventCallback<EVENT> {
    /// Convert `self` to `Option<Callback<EVENT>>`
    fn into_event_callback(self) -> Option<Callback<EVENT>>;
}

impl<EVENT> IntoEventCallback<EVENT> for Callback<EVENT> {
    fn into_event_callback(self) -> Option<Callback<EVENT>> {
        Some(self)
    }
}

impl<EVENT> IntoEventCallback<EVENT> for &Callback<EVENT> {
    fn into_event_callback(self) -> Option<Callback<EVENT>> {
        Some(self.clone())
    }
}

impl<EVENT> IntoEventCallback<EVENT> for Option<Callback<EVENT>> {
    fn into_event_callback(self) -> Option<Callback<EVENT>> {
        self
    }
}

impl<T, EVENT> IntoEventCallback<EVENT> for T
where
    T: Fn(EVENT) + 'static,
{
    fn into_event_callback(self) -> Option<Callback<EVENT>> {
        Some(Callback::from(self))
    }
}

impl<T, EVENT> IntoEventCallback<EVENT> for Option<T>
where
    T: Fn(EVENT) + 'static,
{
    fn into_event_callback(self) -> Option<Callback<EVENT>> {
        Some(Callback::from(self?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_into_event_callback_types() {
        let f = |_: usize| ();
        let cb = Callback::from(f);

        // Callbacks
        let _: Option<Callback<usize>> = cb.clone().into_event_callback();
        let _: Option<Callback<usize>> = (&cb).into_event_callback();
        let _: Option<Callback<usize>> = Some(cb).into_event_callback();

        // Fns
        let _: Option<Callback<usize>> = f.into_event_callback();
        let _: Option<Callback<usize>> = Some(f).into_event_callback();
    }
}
