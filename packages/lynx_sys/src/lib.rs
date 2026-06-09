//! Thin Rust wrappers for Lynx WAMR `env` host functions.
//!
//! Primitive values are copied across the ABI. Host-owned values such as
//! elements, events, arrays, objects, and callbacks are represented by their
//! host **arena id** (`i32`) and carried directly by wrapper structs. A negative
//! id ([`NULL_NODE`]) means "no node"; nullable accessors surface that as
//! `Option<i32>`.

use std::borrow::Cow;
use std::fmt;

pub mod raw;

pub use raw::{HostValueKind, HostValueOut, IntoHostValue, NULL_NODE};

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
        raw::set_attribute(self.raw, key, value);
        Ok(())
    }

    #[inline]
    fn remove_attribute(&self, key: &str) -> Result<()> {
        raw::set_attribute(self.raw, key, None::<i32>);
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
    match raw::get_attribute_by_name(node, "xmlns") {
        HostValueOut::String { bytes, .. } => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| Error::InvalidUtf8("__GetAttributeByName")),
        HostValueOut::Null | HostValueOut::Undefined => Ok(None),
        _ => Ok(None),
    }
}

/// Borrowing variant of [`get_namespace_uri`]: invokes `f` with the recorded
/// namespace URI as a borrowed `&str`, avoiding an owned `String` allocation.
/// The borrow is valid only for the duration of `f`.
#[inline]
pub fn get_namespace_uri_with<R>(node: i32, f: impl FnOnce(Result<Option<&str>>) -> R) -> R {
    raw::get_attribute_by_name_borrow(node, "xmlns", |value| match value {
        raw::AnyBorrow::Str(bytes) => match std::str::from_utf8(bytes) {
            Ok(value) => f(Ok(Some(value))),
            Err(_) => f(Err(Error::InvalidUtf8("__GetAttributeByName"))),
        },
        _ => f(Ok(None)),
    })
}

/// Flushes pending element tree changes.
pub fn commit() -> Result<()> {
    raw::flush_element_tree(None::<i32>, None::<i32>);
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
}

/// Adds an event listener through the host.
pub fn add_event_listener(
    element: i32,
    event_type: &str,
    callback: i32,
    _options: EventListenerOptions,
) -> Result<()> {
    raw::add_event_listener(element, event_type, callback, NULL_NODE);
    Ok(())
}

/// Removes an event listener through the host.
pub fn remove_event_listener(
    element: i32,
    event_type: &str,
    callback: i32,
    _capture: bool,
) -> Result<()> {
    raw::remove_event_listener(element, event_type, callback, NULL_NODE);
    Ok(())
}

/// Returns the current event target while a host callback is being dispatched.
pub fn event_target() -> Option<i32> {
    None
}

/// Returns whether the current event has had its default prevented.
pub fn event_default_prevented() -> bool {
    false
}

/// Invokes a UI method.
pub fn invoke_ui_method(element: i32, method: &str, params: i32, callback: i32) -> Result<()> {
    raw::invoke_ui_method(element, method, params, callback);
    Ok(())
}

/// Creates a timeout.
pub fn set_timeout(callback: i32, delay_ms: i64) -> i64 {
    raw::set_timeout(callback, delay_ms)
}

/// Clears a timeout.
pub fn clear_timeout(timer_id: i64) {
    raw::clear_timeout(timer_id);
}

/// Creates an interval.
pub fn set_interval(callback: i32, delay_ms: i64) -> i64 {
    raw::set_interval(callback, delay_ms)
}

/// Clears an interval.
pub fn clear_interval(timer_id: i64) {
    raw::clear_interval(timer_id);
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
