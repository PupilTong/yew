//! Thin Rust wrappers for Lynx WAMR `env` host functions.
//!
//! Primitive values are copied across the ABI. Host-owned values such as
//! elements, events, arrays, objects, and callbacks are represented by their
//! host **arena id** (`i32`) and carried directly by wrapper structs. A negative
//! id ([`NULL_NODE`]) means "no node"; nullable accessors surface that as
//! `Option<i32>`.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::marker::PhantomData;

pub mod css;
pub mod raw;

pub use raw::{
    EVENT_FLAG_BUBBLES, EVENT_FLAG_CANCELABLE, EVENT_FLAG_CAPTURE, EVENT_FLAG_COMPOSED,
    LISTENER_FLAG_CAPTURE, LISTENER_FLAG_ONCE, LISTENER_FLAG_PASSIVE, NULL_NODE,
};

/// Error returned by binding convenience wrappers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The host returned no node (a negative arena id) where one was expected.
    NullNode(&'static str),
    /// The host string return did not fit in the guest-provided buffer.
    StringBufferTooSmall {
        /// Binding that produced the string.
        binding: &'static str,
        /// Required UTF-8 byte length returned by native.
        required: usize,
        /// Guest-provided output capacity.
        capacity: usize,
    },
    /// The host returned bytes that were not valid UTF-8.
    InvalidUtf8(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullNode(binding) => write!(f, "{binding} returned no node"),
            Self::StringBufferTooSmall {
                binding,
                required,
                capacity,
            } => write!(
                f,
                "{binding} returned {required} bytes, but buffer capacity is {capacity}"
            ),
            Self::InvalidUtf8(binding) => write!(f, "{binding} returned invalid UTF-8"),
        }
    }
}

impl std::error::Error for Error {}

/// Binding result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Host timer id returned by `setTimeout` and `setInterval`.
pub type TimerId = i64;

struct TimerEntry {
    repeating: bool,
    callback: Box<dyn FnMut()>,
}

thread_local! {
    static TIMER_CALLBACKS: RefCell<HashMap<TimerId, TimerEntry>> = RefCell::new(HashMap::new());
    static CANCELLED_TIMERS: RefCell<HashSet<TimerId>> = RefCell::new(HashSet::new());
}

fn timer_callback_index() -> i32 {
    __lynx_sys_timer_dispatch as usize as i32
}

fn store_timer_callback(timer_id: TimerId, entry: TimerEntry) {
    if timer_id <= 0 {
        return;
    }
    CANCELLED_TIMERS.with(|timers| {
        timers.borrow_mut().remove(&timer_id);
    });
    TIMER_CALLBACKS.with(|callbacks| {
        callbacks.borrow_mut().insert(timer_id, entry);
    });
}

fn remove_timer_callback(timer_id: TimerId) {
    if timer_id <= 0 {
        return;
    }
    TIMER_CALLBACKS.with(|callbacks| {
        callbacks.borrow_mut().remove(&timer_id);
    });
    CANCELLED_TIMERS.with(|timers| {
        timers.borrow_mut().insert(timer_id);
    });
}

fn dispatch_timer(timer_id: TimerId) {
    let Some(mut entry) =
        TIMER_CALLBACKS.with(|callbacks| callbacks.borrow_mut().remove(&timer_id))
    else {
        return;
    };

    (entry.callback)();

    if entry.repeating {
        let cancelled = CANCELLED_TIMERS.with(|timers| timers.borrow_mut().remove(&timer_id));
        if !cancelled {
            TIMER_CALLBACKS.with(|callbacks| {
                callbacks.borrow_mut().insert(timer_id, entry);
            });
        }
    } else {
        CANCELLED_TIMERS.with(|timers| {
            timers.borrow_mut().remove(&timer_id);
        });
    }
}

#[no_mangle]
extern "C" fn __lynx_sys_timer_dispatch(timer_id: i32) {
    dispatch_timer(timer_id.into());
}

#[inline]
fn string_return(
    binding: &'static str,
    call: impl Fn(*mut u8, i32) -> i32,
) -> Result<Option<String>> {
    match raw::string_out_call(call) {
        raw::StringOut::Absent => Ok(None),
        raw::StringOut::Bytes(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| Error::InvalidUtf8(binding)),
        raw::StringOut::Overflow { required, capacity } => Err(Error::StringBufferTooSmall {
            binding,
            required,
            capacity,
        }),
    }
}

/// Calls a string-returning binding and invokes `f` with the borrowed UTF-8
/// result without allocating an owned `String`. `f` receives `Ok(None)` when the
/// host reports the value absent, or `Err(InvalidUtf8)` when the bytes are not
/// valid UTF-8. The borrowed `&str` is valid only for the duration of `f`.
#[inline]
fn string_return_with<R>(
    binding: &'static str,
    call: impl Fn(*mut u8, i32) -> i32,
    f: impl FnOnce(Result<Option<&str>>) -> R,
) -> R {
    raw::string_borrow_call(call, |bytes| match bytes {
        None => f(Ok(None)),
        Some(bytes) => match std::str::from_utf8(bytes) {
            Ok(value) => f(Ok(Some(value))),
            Err(_) => f(Err(Error::InvalidUtf8(binding))),
        },
    })
}

/// Requires a non-negative host arena id, mapping the negative "no node"
/// sentinel to an error.
#[inline]
fn required_ref(binding: &'static str, raw: i32) -> Result<i32> {
    if raw < 0 {
        Err(Error::NullNode(binding))
    } else {
        Ok(raw)
    }
}

/// Host element wrapper.
///
/// The wrapper stores the host arena id directly. It does not allocate a
/// guest-side id or participate in a guest-side node registry.
#[derive(Clone)]
pub struct Element {
    raw: i32,
}

impl Element {
    /// Creates an element by tag name.
    pub fn new(tag: &str) -> Result<Self> {
        let raw = raw::create_element(tag);
        required_ref("__CreateElement", raw).map(Self::from_raw_unchecked)
    }

    /// Creates an element and records the namespace URI as `xmlns`.
    pub fn new_ns(namespace: &str, tag: &str) -> Result<Self> {
        let element = Self::new(tag)?;
        element.set_attribute("xmlns", namespace)?;
        Ok(element)
    }

    /// Wraps a host arena id, returning `None` for the "no node" sentinel.
    #[inline]
    pub fn from_raw(raw: i32) -> Option<Self> {
        if raw < 0 {
            None
        } else {
            Some(Self::from_raw_unchecked(raw))
        }
    }

    /// Wraps a host arena id without checking for the "no node" sentinel.
    #[inline(always)]
    pub fn from_raw_unchecked(raw: i32) -> Self {
        Self { raw }
    }

    /// Returns the host arena id carried by this wrapper.
    #[inline(always)]
    pub fn raw(&self) -> i32 {
        self.raw
    }

    /// Returns the host arena id. Kept for existing Yew call sites.
    #[inline(always)]
    pub fn id(&self) -> i32 {
        self.raw
    }

    /// Returns the host's stable unique id for this element, when needed for diagnostics.
    pub fn unique_id(&self) -> i64 {
        raw::get_element_unique_id(self.raw)
    }

    /// Consumes the wrapper and returns the host arena id.
    #[inline(always)]
    pub fn into_raw(self) -> i32 {
        self.raw
    }
}

impl fmt::Debug for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Element")
            .field("raw", &self.raw)
            .field("unique_id", &self.unique_id())
            .finish()
    }
}

/// Host text-node wrapper.
#[derive(Clone)]
pub struct Text {
    raw: i32,
}

impl Text {
    /// Creates a raw text node.
    pub fn new(text: &str) -> Result<Self> {
        let raw = raw::create_raw_text(text);
        required_ref("__CreateRawText", raw).map(Self::from_raw_unchecked)
    }

    /// Wraps a host arena id, returning `None` for the "no node" sentinel.
    #[inline]
    pub fn from_raw(raw: i32) -> Option<Self> {
        if raw < 0 {
            None
        } else {
            Some(Self::from_raw_unchecked(raw))
        }
    }

    /// Wraps a host arena id without checking for the "no node" sentinel.
    #[inline(always)]
    pub fn from_raw_unchecked(raw: i32) -> Self {
        Self { raw }
    }

    /// Returns the host arena id carried by this wrapper.
    #[inline(always)]
    pub fn raw(&self) -> i32 {
        self.raw
    }

    /// Returns the host arena id. Kept for existing Yew call sites.
    #[inline(always)]
    pub fn id(&self) -> i32 {
        self.raw
    }

    /// Consumes the wrapper and returns the host arena id.
    #[inline(always)]
    pub fn into_raw(self) -> i32 {
        self.raw
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Text").field("raw", &self.raw).finish()
    }
}

/// Owning host event wrapper.
///
/// `Event` owns one id in the host event arena. Dropping it calls
/// `binding__DropEvent`. Use [`Event::as_ref`] or
/// [`Event::with_borrowed_raw`] when an arena id should only be borrowed.
pub struct Event {
    raw: i32,
}

impl Event {
    /// Creates a host event in the typed event arena.
    pub fn new(event_type: i32, name: &str, flags: i32) -> Result<Self> {
        let raw =
            raw::create_event(event_type, name, flags).ok_or(Error::NullNode("__CreateEvent"))?;
        Ok(Self::from_raw_unchecked(raw))
    }

    /// Takes ownership of a host event arena id.
    ///
    /// This is the right constructor for ids returned by `__CreateEvent` and
    /// for event ids delivered to wasm callbacks when ownership is transferred
    /// to the guest.
    #[inline]
    pub fn from_raw(raw: i32) -> Option<Self> {
        if raw < 0 {
            None
        } else {
            Some(Self::from_raw_unchecked(raw))
        }
    }

    /// Takes ownership of a host event arena id without checking the null sentinel.
    #[inline(always)]
    pub fn from_raw_unchecked(raw: i32) -> Self {
        Self { raw }
    }

    /// Returns the host event arena id.
    #[inline(always)]
    pub fn raw(&self) -> i32 {
        self.raw
    }

    /// Borrows this event without taking ownership of the arena id.
    #[inline(always)]
    pub fn as_ref(&self) -> EventRef<'_> {
        EventRef::from_raw_unchecked(self.raw)
    }

    /// Borrows a raw host event arena id for the duration of `f`.
    ///
    /// This does not call `binding__DropEvent`. Prefer [`Event::from_raw`] when
    /// the raw id is handed to wasm with ownership.
    #[inline]
    pub fn with_borrowed_raw<R>(raw: i32, f: impl FnOnce(EventRef<'_>) -> R) -> Option<R> {
        if raw < 0 {
            None
        } else {
            Some(f(EventRef::from_raw_unchecked(raw)))
        }
    }

    /// Consumes the wrapper without dropping the host event.
    ///
    /// The caller becomes responsible for eventually calling [`drop_event`].
    #[inline(always)]
    pub fn into_raw(self) -> i32 {
        let raw = self.raw;
        std::mem::forget(self);
        raw
    }

    /// Dispatches this event on an element.
    pub fn dispatch_on(&self, element: i32) -> bool {
        self.as_ref().dispatch_on(element)
    }

    /// Stops further propagation after the current target phase.
    pub fn stop_propagation(&self) {
        self.as_ref().stop_propagation();
    }

    /// Stops subsequent listeners on the same target and further propagation.
    pub fn stop_immediate_propagation(&self) {
        self.as_ref().stop_immediate_propagation();
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        if self.raw >= 0 {
            raw::drop_event(self.raw);
        }
    }
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Event").field("raw", &self.raw).finish()
    }
}

/// Borrowed host event arena id.
///
/// `EventRef` never releases the event arena entry. It is useful when an API
/// needs event operations but ownership stays with another [`Event`] wrapper or
/// with the caller that supplied the raw id.
#[derive(Clone, Copy)]
pub struct EventRef<'a> {
    raw: i32,
    _marker: PhantomData<&'a Event>,
}

impl<'a> EventRef<'a> {
    #[inline(always)]
    fn from_raw_unchecked(raw: i32) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }

    /// Returns the host event arena id.
    #[inline(always)]
    pub fn raw(&self) -> i32 {
        self.raw
    }

    /// Dispatches this event on an element.
    pub fn dispatch_on(&self, element: i32) -> bool {
        raw::dispatch_event(element, self.raw)
    }

    /// Stops further propagation after the current target phase.
    pub fn stop_propagation(&self) {
        raw::stop_propagation(self.raw);
    }

    /// Stops subsequent listeners on the same target and further propagation.
    pub fn stop_immediate_propagation(&self) {
        raw::stop_immediate_propagation(self.raw);
    }
}

impl fmt::Debug for EventRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventRef").field("raw", &self.raw).finish()
    }
}

/// Shared node operations.
pub trait NodeOps {
    /// Returns the host arena id for this node.
    fn id(&self) -> i32;
}

impl NodeOps for Element {
    fn id(&self) -> i32 {
        self.raw
    }
}

impl NodeOps for Text {
    fn id(&self) -> i32 {
        self.raw
    }
}

impl NodeOps for i32 {
    fn id(&self) -> i32 {
        *self
    }
}

/// Element attribute operations used by Yew.
pub trait ElementOps {
    /// Sets an attribute to a UTF-8 string value.
    fn set_attribute(&self, key: &str, value: &str) -> Result<()>;

    /// Removes an attribute by writing a copied `null` value.
    fn remove_attribute(&self, key: &str) -> Result<()>;
}

impl ElementOps for Element {
    #[inline]
    fn set_attribute(&self, key: &str, value: &str) -> Result<()> {
        if key == "class" {
            raw::set_classes(self.raw, value);
            return Ok(());
        }
        if key == "style" {
            raw::set_inline_style_text(self.raw, value);
            return Ok(());
        }
        raw::set_string_attribute(self.raw, key, value);
        Ok(())
    }

    #[inline]
    fn remove_attribute(&self, key: &str) -> Result<()> {
        if key == "class" {
            raw::set_classes(self.raw, "");
            return Ok(());
        }
        raw::remove_attribute(self.raw, key);
        Ok(())
    }
}

/// Appends a child to a parent element.
#[inline]
pub fn append_child(parent: i32, child: i32) -> Result<i32> {
    Ok(raw::append_element(parent, child))
}

/// Removes a child from a parent element.
#[inline]
pub fn remove_child(parent: i32, child: i32) -> Result<i32> {
    Ok(raw::remove_element(parent, child))
}

/// Drops a host element arena id created by the `__Create*` bindings.
pub fn drop_element(element: i32) {
    raw::drop_element(element);
}

/// Drops a host event arena id created by `__CreateEvent` or handed to wasm.
pub fn drop_event(event: i32) {
    raw::drop_event(event);
}

/// Inserts a child before `ref_child`, or appends it when `ref_child` is `None`.
#[inline]
pub fn insert_before(parent: i32, node: i32, ref_child: Option<i32>) -> Result<i32> {
    Ok(raw::insert_element_before(parent, node, ref_child))
}

/// Returns the first child of an element.
#[inline]
pub fn get_first_child(parent: i32) -> Option<i32> {
    raw::first_element(parent)
}

/// Returns the last child of an element.
#[inline]
pub fn get_last_child(parent: i32) -> Option<i32> {
    raw::last_element(parent)
}

/// Returns the next sibling of a node.
#[inline]
pub fn get_next_sibling(node: i32) -> Option<i32> {
    raw::next_element(node)
}

/// Returns the parent element of a node.
#[inline]
pub fn get_parent_element(node: i32) -> Option<i32> {
    raw::get_parent(node)
}

/// Returns the host tag name.
pub fn get_tag(node: i32) -> Result<Option<String>> {
    string_return("__GetTag", |ptr, max| raw::get_tag(node, ptr, max))
}

/// Borrowing variant of [`get_tag`]: invokes `f` with the tag name as a borrowed
/// `&str`, avoiding an owned `String` allocation. The borrow is valid only for
/// the duration of `f`.
#[inline]
pub fn get_tag_with<R>(node: i32, f: impl FnOnce(Result<Option<&str>>) -> R) -> R {
    string_return_with("__GetTag", |ptr, max| raw::get_tag(node, ptr, max), f)
}

/// Returns the namespace URI recorded on an element, if present.
pub fn get_namespace_uri(node: i32) -> Result<Option<String>> {
    string_return("__GetStringAttributeByName", |ptr, max| {
        raw::get_string_attribute_by_name(node, "xmlns", ptr, max)
    })
}

/// Borrowing variant of [`get_namespace_uri`]: invokes `f` with the recorded
/// namespace URI as a borrowed `&str`, avoiding an owned `String` allocation.
/// The borrow is valid only for the duration of `f`.
#[inline]
pub fn get_namespace_uri_with<R>(node: i32, f: impl FnOnce(Result<Option<&str>>) -> R) -> R {
    string_return_with(
        "__GetStringAttributeByName",
        |ptr, max| raw::get_string_attribute_by_name(node, "xmlns", ptr, max),
        f,
    )
}

/// Flushes pending element tree changes.
pub fn commit() -> Result<()> {
    raw::flush_element_tree(None);
    Ok(())
}

/// Event listener options.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EventListenerOptions {
    capture: bool,
    passive: bool,
    once: bool,
}

impl EventListenerOptions {
    /// Creates empty listener options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables capture.
    pub fn capture(mut self) -> Self {
        self.capture = true;
        self
    }

    /// Enables passive listener semantics.
    pub fn passive(mut self) -> Self {
        self.passive = true;
        self
    }

    /// Enables one-shot listener semantics.
    pub fn once(mut self) -> Self {
        self.once = true;
        self
    }

    fn bits(self) -> i32 {
        (if self.capture {
            LISTENER_FLAG_CAPTURE
        } else {
            0
        }) | (if self.once { LISTENER_FLAG_ONCE } else { 0 })
            | (if self.passive {
                LISTENER_FLAG_PASSIVE
            } else {
                0
            })
    }
}

/// Adds an event listener through the host.
///
/// `callback` is a wasm-internal handler id. Native invokes it with one owned
/// event arena id; the callback should wrap that id with [`Event::from_raw`] or
/// release it with [`drop_event`].
pub fn add_event_listener(
    element: i32,
    event_type: &str,
    callback: i32,
    options: EventListenerOptions,
) -> Result<()> {
    raw::add_event_listener(element, event_type, callback, options.bits());
    Ok(())
}

/// Removes an event listener through the host.
pub fn remove_event_listener(
    element: i32,
    event_type: &str,
    callback: i32,
    capture: bool,
) -> Result<()> {
    let options = if capture { LISTENER_FLAG_CAPTURE } else { 0 };
    raw::remove_event_listener(element, event_type, callback, options);
    Ok(())
}

/// Removes an event listener through the host using the full option bitset.
pub fn remove_event_listener_with_options(
    element: i32,
    event_type: &str,
    callback: i32,
    options: EventListenerOptions,
) -> Result<()> {
    raw::remove_event_listener(element, event_type, callback, options.bits());
    Ok(())
}

/// Dispatches a raw event arena id on an element.
pub fn dispatch_event(element: i32, event: i32) -> bool {
    raw::dispatch_event(element, event)
}

/// Stops further propagation for a raw event arena id.
pub fn stop_propagation(event: i32) {
    raw::stop_propagation(event);
}

/// Stops subsequent listeners on the same target and further propagation.
pub fn stop_immediate_propagation(event: i32) {
    raw::stop_immediate_propagation(event);
}

/// Returns the host event type, such as `tap` or `click`.
pub fn event_type(event: i32) -> Result<Option<String>> {
    string_return("__GetEventType", |ptr, max| {
        raw::get_event_type(event, ptr, max)
    })
}

/// Borrowing variant of [`event_type`].
#[inline]
pub fn event_type_with<R>(event: i32, f: impl FnOnce(Result<Option<&str>>) -> R) -> R {
    string_return_with(
        "__GetEventType",
        |ptr, max| raw::get_event_type(event, ptr, max),
        f,
    )
}

/// Returns the unique id of the event's current target.
pub fn event_current_target_unique_id(event: i32) -> Option<i64> {
    raw::get_event_current_target_unique_id(event)
}

/// Returns the current event target while a host callback is being dispatched.
pub fn event_target() -> Option<i32> {
    None
}

/// Returns whether the current event has had its default prevented.
pub fn event_default_prevented() -> bool {
    false
}

/// Creates a timeout and invokes `callback` once after `delay_ms`.
pub fn set_timeout<F>(callback: F, delay_ms: i64) -> TimerId
where
    F: FnOnce() + 'static,
{
    let timer_id = raw::set_timeout(timer_callback_index(), delay_ms);
    let mut callback = Some(callback);
    store_timer_callback(
        timer_id,
        TimerEntry {
            repeating: false,
            callback: Box::new(move || {
                if let Some(callback) = callback.take() {
                    callback();
                }
            }),
        },
    );
    timer_id
}

/// Clears a timeout.
pub fn clear_timeout(timer_id: TimerId) {
    raw::clear_timeout(timer_id);
    remove_timer_callback(timer_id);
}

/// Creates an interval and invokes `callback` repeatedly every `delay_ms`.
pub fn set_interval<F>(callback: F, delay_ms: i64) -> TimerId
where
    F: FnMut() + 'static,
{
    let timer_id = raw::set_interval(timer_callback_index(), delay_ms);
    store_timer_callback(
        timer_id,
        TimerEntry {
            repeating: true,
            callback: Box::new(callback),
        },
    );
    timer_id
}

/// Clears an interval.
pub fn clear_interval(timer_id: TimerId) {
    raw::clear_interval(timer_id);
    remove_timer_callback(timer_id);
}

/// Convenience conversion from a host arena id.
impl From<i32> for Element {
    fn from(raw: i32) -> Self {
        Self::from_raw_unchecked(raw)
    }
}

/// Debug helper for nullable host strings.
pub fn display_optional_string(value: Option<String>) -> Cow<'static, str> {
    value.map(Cow::Owned).unwrap_or(Cow::Borrowed(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_from_raw_maps_negative_to_none() {
        assert!(Event::from_raw(NULL_NODE).is_none());
        assert!(Event::from_raw(-42).is_none());
    }

    #[test]
    fn event_into_raw_transfers_owned_id_without_drop() {
        let event = Event::from_raw_unchecked(17);
        assert_eq!(event.as_ref().raw(), 17);
        assert_eq!(event.into_raw(), 17);
    }

    #[test]
    fn borrowed_raw_event_is_closure_scoped() {
        let seen = Event::with_borrowed_raw(29, |event| {
            let copy = event;
            (event.raw(), copy.raw())
        });
        assert_eq!(seen, Some((29, 29)));
        assert!(Event::with_borrowed_raw(NULL_NODE, |_| ()).is_none());
    }
}
