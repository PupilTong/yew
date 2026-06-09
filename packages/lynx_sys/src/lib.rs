//! Thin Rust wrappers for Lynx WAMR `env` host functions.
//!
//! Primitive values are copied across the ABI. Host-owned values such as
//! elements, events, arrays, objects, and callbacks are represented by
//! [`ExternRef`] and carried directly by wrapper structs.

use std::borrow::Cow;
use std::fmt;

pub mod raw;

pub use raw::{ExternRef, HostValue, HostValueKind, HostValueOut};

/// Error returned by binding convenience wrappers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The host returned a null externref where a node was expected.
    NullExternRef(&'static str),
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
            Self::NullExternRef(binding) => write!(f, "{binding} returned null externref"),
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

#[inline]
fn required_ref(binding: &'static str, raw: ExternRef) -> Result<ExternRef> {
    if raw.is_null() {
        Err(Error::NullExternRef(binding))
    } else {
        Ok(raw)
    }
}

/// Host element wrapper.
///
/// The wrapper stores the host `externref` directly. It does not allocate a
/// guest-side id or participate in a guest-side node registry.
#[derive(Clone)]
pub struct Element {
    raw: ExternRef,
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

    /// Wraps a non-null host reference.
    #[inline]
    pub fn from_raw(raw: ExternRef) -> Option<Self> {
        if raw.is_null() {
            None
        } else {
            Some(Self::from_raw_unchecked(raw))
        }
    }

    /// Wraps a host reference without checking for null.
    #[inline(always)]
    pub fn from_raw_unchecked(raw: ExternRef) -> Self {
        Self { raw }
    }

    /// Returns the raw host reference carried by this wrapper.
    #[inline(always)]
    pub fn raw(&self) -> ExternRef {
        self.raw
    }

    /// Returns the raw host reference. Kept for existing Yew call sites.
    #[inline(always)]
    pub fn id(&self) -> ExternRef {
        self.raw
    }

    /// Returns the host's stable unique id for this element, when needed for diagnostics.
    pub fn unique_id(&self) -> i64 {
        raw::get_element_unique_id(self.raw)
    }

    /// Consumes the wrapper and returns the host reference.
    #[inline(always)]
    pub fn into_raw(self) -> ExternRef {
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
    raw: ExternRef,
}

impl Text {
    /// Creates a raw text node.
    pub fn new(text: &str) -> Result<Self> {
        let raw = raw::create_raw_text(text);
        required_ref("__CreateRawText", raw).map(Self::from_raw_unchecked)
    }

    /// Wraps a non-null host reference.
    #[inline]
    pub fn from_raw(raw: ExternRef) -> Option<Self> {
        if raw.is_null() {
            None
        } else {
            Some(Self::from_raw_unchecked(raw))
        }
    }

    /// Wraps a host reference without checking for null.
    #[inline(always)]
    pub fn from_raw_unchecked(raw: ExternRef) -> Self {
        Self { raw }
    }

    /// Returns the raw host reference carried by this wrapper.
    #[inline(always)]
    pub fn raw(&self) -> ExternRef {
        self.raw
    }

    /// Returns the raw host reference. Kept for existing Yew call sites.
    #[inline(always)]
    pub fn id(&self) -> ExternRef {
        self.raw
    }

    /// Consumes the wrapper and returns the host reference.
    #[inline(always)]
    pub fn into_raw(self) -> ExternRef {
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
    /// Returns the raw host reference for this node.
    fn id(&self) -> ExternRef;
}

impl NodeOps for Element {
    fn id(&self) -> ExternRef {
        self.raw
    }
}

impl NodeOps for Text {
    fn id(&self) -> ExternRef {
        self.raw
    }
}

impl NodeOps for ExternRef {
    fn id(&self) -> ExternRef {
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
        raw::set_attribute(self.raw, HostValue::String(key), HostValue::String(value));
        Ok(())
    }

    #[inline]
    fn remove_attribute(&self, key: &str) -> Result<()> {
        raw::set_attribute(self.raw, HostValue::String(key), HostValue::Null);
        Ok(())
    }
}

/// Appends a child to a parent element.
#[inline]
pub fn append_child(parent: ExternRef, child: ExternRef) -> Result<ExternRef> {
    Ok(raw::append_element(parent, child))
}

/// Removes a child from a parent element.
#[inline]
pub fn remove_child(parent: ExternRef, child: ExternRef) -> Result<ExternRef> {
    Ok(raw::remove_element(parent, child))
}

/// Drops a host element number-id created by the `__Create*` bindings.
pub fn drop_element(element: ExternRef) {
    raw::drop_element(element);
}

/// Inserts a child before `ref_child`, or appends it when `ref_child` is `None`.
#[inline]
pub fn insert_before(
    parent: ExternRef,
    node: ExternRef,
    ref_child: Option<ExternRef>,
) -> Result<ExternRef> {
    let ref_child = ref_child
        .map(HostValue::ExternRef)
        .unwrap_or(HostValue::Null);
    Ok(raw::insert_element_before(parent, node, ref_child))
}

/// Returns the first child of an element.
#[inline]
pub fn get_first_child(parent: ExternRef) -> Option<ExternRef> {
    let raw = raw::first_element(parent);
    (!raw.is_null()).then_some(raw)
}

/// Returns the last child of an element.
#[inline]
pub fn get_last_child(parent: ExternRef) -> Option<ExternRef> {
    let raw = raw::last_element(parent);
    (!raw.is_null()).then_some(raw)
}

/// Returns the next sibling of a node.
#[inline]
pub fn get_next_sibling(node: ExternRef) -> Option<ExternRef> {
    let raw = raw::next_element(node);
    (!raw.is_null()).then_some(raw)
}

/// Returns the parent element of a node.
#[inline]
pub fn get_parent_element(node: ExternRef) -> Option<ExternRef> {
    let raw = raw::get_parent(node);
    (!raw.is_null()).then_some(raw)
}

/// Returns the host tag name.
pub fn get_tag(node: ExternRef) -> Result<Option<String>> {
    string_return("__GetTag", |ptr, max| raw::get_tag(node, ptr, max))
}

/// Borrowing variant of [`get_tag`]: invokes `f` with the tag name as a borrowed
/// `&str`, avoiding an owned `String` allocation. The borrow is valid only for
/// the duration of `f`.
#[inline]
pub fn get_tag_with<R>(node: ExternRef, f: impl FnOnce(Result<Option<&str>>) -> R) -> R {
    string_return_with("__GetTag", |ptr, max| raw::get_tag(node, ptr, max), f)
}

/// Returns the namespace URI recorded on an element, if present.
pub fn get_namespace_uri(node: ExternRef) -> Result<Option<String>> {
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
pub fn get_namespace_uri_with<R>(node: ExternRef, f: impl FnOnce(Result<Option<&str>>) -> R) -> R {
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
    raw::flush_element_tree(HostValue::Null, HostValue::Null);
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
    element: ExternRef,
    event_type: &str,
    callback: ExternRef,
    _options: EventListenerOptions,
) -> Result<()> {
    raw::add_event_listener(element, event_type, callback, ExternRef::null());
    Ok(())
}

/// Removes an event listener through the host.
pub fn remove_event_listener(
    element: ExternRef,
    event_type: &str,
    callback: ExternRef,
    _capture: bool,
) -> Result<()> {
    raw::remove_event_listener(element, event_type, callback, ExternRef::null());
    Ok(())
}

/// Returns the current event target while a host callback is being dispatched.
pub fn event_target() -> Option<ExternRef> {
    None
}

/// Returns whether the current event has had its default prevented.
pub fn event_default_prevented() -> bool {
    false
}

/// Invokes a UI method.
pub fn invoke_ui_method(
    element: ExternRef,
    method: &str,
    params: ExternRef,
    callback: ExternRef,
) -> Result<()> {
    raw::invoke_ui_method(element, method, params, callback);
    Ok(())
}

/// Creates a timeout.
pub fn set_timeout(callback: ExternRef, delay_ms: i64) -> i64 {
    raw::set_timeout(callback, delay_ms)
}

/// Clears a timeout.
pub fn clear_timeout(timer_id: i64) {
    raw::clear_timeout(timer_id);
}

/// Creates an interval.
pub fn set_interval(callback: ExternRef, delay_ms: i64) -> i64 {
    raw::set_interval(callback, delay_ms)
}

/// Clears an interval.
pub fn clear_interval(timer_id: i64) {
    raw::clear_interval(timer_id);
}

/// Convenience conversion from a raw host reference.
impl From<ExternRef> for Element {
    fn from(raw: ExternRef) -> Self {
        Self::from_raw_unchecked(raw)
    }
}

/// Convenience conversion from a raw host reference.
impl From<Element> for ExternRef {
    fn from(element: Element) -> Self {
        element.raw
    }
}

/// Convenience conversion from a raw host reference.
impl From<&Element> for ExternRef {
    fn from(element: &Element) -> Self {
        element.raw
    }
}

/// Debug helper for nullable host strings.
pub fn display_optional_string(value: Option<String>) -> Cow<'static, str> {
    value.map(Cow::Owned).unwrap_or(Cow::Borrowed(""))
}
