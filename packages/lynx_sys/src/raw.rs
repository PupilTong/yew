//! Raw wrappers for the WAMR `env` host-function ABI.

use std::cell::RefCell;

/// Raw host value kind for dynamic `any` arguments and returns.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostValueKind {
    /// Undefined dynamic value.
    Undefined = 0,
    /// Null.
    Null = 1,
    /// Boolean.
    Bool = 2,
    /// Number.
    Number = 3,
    /// UTF-8 string in wasm memory.
    String = 4,
    /// Host-owned reference.
    ExternRef = 5,
}

/// Sentinel host handle meaning "no node".
///
/// The host allocates every element/node in an arena and returns its
/// non-negative arena id; a negative id (this value) means "absent". Wrappers
/// that can return "no node" surface this as [`Option::None`] via [`node`].
pub const NULL_NODE: i32 = -1;

/// Converts a raw host arena id into `Option`: a non-negative id is a real node,
/// a negative id is "absent".
#[inline(always)]
fn node(raw: i32) -> Option<i32> {
    (raw >= 0).then_some(raw)
}

/// Dynamic host value used for copied ABI values and host references.
#[derive(Debug, Clone, Copy)]
pub enum HostValue<'a> {
    /// Undefined.
    Undefined,
    /// Null.
    Null,
    /// Boolean.
    Bool(bool),
    /// Number.
    Number(f64),
    /// UTF-8 string.
    String(&'a str),
    /// Host-owned reference (arena id).
    ExternRef(i32),
}

impl HostValue<'_> {
    #[inline(always)]
    fn into_raw_parts(self) -> (i32, f64, i32, i32, i32) {
        match self {
            Self::Undefined => (HostValueKind::Undefined as i32, 0.0, 0, 0, NULL_NODE),
            Self::Null => (HostValueKind::Null as i32, 0.0, 0, 0, NULL_NODE),
            Self::Bool(value) => (
                HostValueKind::Bool as i32,
                if value { 1.0 } else { 0.0 },
                0,
                0,
                NULL_NODE,
            ),
            Self::Number(value) => (HostValueKind::Number as i32, value, 0, 0, NULL_NODE),
            Self::String(value) => (
                HostValueKind::String as i32,
                0.0,
                value.as_ptr() as i32,
                value.len() as i32,
                NULL_NODE,
            ),
            Self::ExternRef(value) => (HostValueKind::ExternRef as i32, 0.0, 0, 0, value),
        }
    }
}

/// Dynamic host value return descriptor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct HostValueAbiOut {
    kind: i32,
    bool_value: i32,
    number_value: f64,
    string_required_length: i32,
    string_written_length: i32,
}

/// Decoded dynamic host return value.
#[derive(Debug, Clone, PartialEq)]
pub enum HostValueOut {
    /// Undefined.
    Undefined,
    /// Null.
    Null,
    /// Boolean.
    Bool(bool),
    /// Number.
    Number(f64),
    /// UTF-8 string.
    String {
        /// String bytes copied into guest memory.
        bytes: Vec<u8>,
        /// Required byte length reported by native.
        required_len: usize,
    },
    /// Host-owned reference (arena id).
    ExternRef(i32),
}

/// Initial capacity for the reusable host-string scratch buffer.
///
/// The buffer is reused across calls and grown on demand, so steady-state
/// string returns allocate nothing for the transfer; 1 KiB covers tag names,
/// attribute values, ids, and namespace URIs without growing on the first call.
const SCRATCH_INITIAL_CAPACITY: usize = 1024;

thread_local! {
    /// Reusable scratch buffer backing every host string-out call on this thread.
    ///
    /// The guest is single-threaded `wasm32-wasip1`, so this is effectively a
    /// global with one owner. Reusing the allocation avoids a per-call `malloc`
    /// + zero-fill + `free`; results are copied out at their exact size.
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Result of [`string_out_call`].
pub(crate) enum StringOut {
    /// The host reported the value is absent (negative sentinel).
    Absent,
    /// Exact-capacity bytes copied out of the scratch buffer.
    Bytes(Vec<u8>),
    /// The host re-reported a required length larger than the grown capacity
    /// (host misreporting); surfaced so callers can raise an error.
    Overflow {
        /// Required byte length reported by the host on retry.
        required: usize,
        /// Capacity offered on retry.
        capacity: usize,
    },
}

/// Grows `buf` so the host has at least `min` writable bytes, without
/// zero-filling, and sets the logical length to the full capacity so
/// `as_mut_ptr()` + `len()` describe the whole writable window.
fn ensure_capacity(buf: &mut Vec<u8>, min: usize) {
    if buf.capacity() < min {
        buf.reserve(min - buf.len());
    }
    // SAFETY: `u8` has no invalid bit patterns, and we never read past the
    // `required`/`written` count the host reports, so the uninitialized tail is
    // never observed. The pointer/length pair handed to the host stays within
    // the single allocation backing `buf`.
    unsafe {
        buf.set_len(buf.capacity());
    }
}

/// Runs `f` with the thread-local scratch buffer (grown to at least
/// `min_capacity`), falling back to a one-off buffer if the scratch is already
/// borrowed — i.e. a host call re-entered a string-returning binding on this
/// thread. That is not expected for these accessors, but we handle it without
/// panicking.
fn with_scratch<R>(min_capacity: usize, f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
    SCRATCH.with(|cell| match cell.try_borrow_mut() {
        Ok(mut buf) => {
            ensure_capacity(&mut buf, min_capacity);
            f(&mut buf)
        }
        Err(_) => {
            let mut buf = Vec::new();
            ensure_capacity(&mut buf, min_capacity);
            f(&mut buf)
        }
    })
}

#[inline(always)]
fn string_parts(value: &str) -> (i32, i32) {
    (value.as_ptr() as i32, value.len() as i32)
}

/// Drives one host string-out call against the reusable scratch buffer.
///
/// `call(ptr, max_len)` returns the host-reported *required* byte length, or a
/// negative sentinel meaning "absent". When the first capacity is too small the
/// buffer grows to exactly `required` and the call is retried once (the host's
/// `required` is authoritative). Bytes are copied out at their exact size.
pub(crate) fn string_out_call(call: impl Fn(*mut u8, i32) -> i32) -> StringOut {
    with_scratch(SCRATCH_INITIAL_CAPACITY, |buf| {
        let required = call(buf.as_mut_ptr(), buf.len() as i32);
        if required < 0 {
            return StringOut::Absent;
        }
        let required = required as usize;
        if required <= buf.len() {
            return StringOut::Bytes(buf[..required].to_vec());
        }

        // Too small: grow to fit and let the host write the full payload once more.
        ensure_capacity(buf, required);
        let required = call(buf.as_mut_ptr(), buf.len() as i32);
        if required < 0 {
            return StringOut::Absent;
        }
        let required = required as usize;
        if required > buf.len() {
            return StringOut::Overflow {
                required,
                capacity: buf.len(),
            };
        }
        StringOut::Bytes(buf[..required].to_vec())
    })
}

/// Decodes a dynamic return that carries no string payload, without touching
/// the scratch buffer.
fn decode_non_string(raw_ref: i32, out: &HostValueAbiOut) -> HostValueOut {
    match out.kind {
        value if value == HostValueKind::Null as i32 => HostValueOut::Null,
        value if value == HostValueKind::Bool as i32 => HostValueOut::Bool(out.bool_value != 0),
        value if value == HostValueKind::Number as i32 => HostValueOut::Number(out.number_value),
        value if value == HostValueKind::ExternRef as i32 => HostValueOut::ExternRef(raw_ref),
        _ => HostValueOut::Undefined,
    }
}

/// Drives a dynamic (`any`) host return against the reusable scratch buffer.
///
/// Only the `String` kind reads the buffer; every other kind decodes from the
/// out-struct and allocates nothing. The `String` path grows + retries once if
/// the host's required length exceeds the offered capacity, then copies the
/// written bytes out at their exact size.
fn any_return(call: impl Fn(*mut HostValueAbiOut, *mut u8, i32) -> i32) -> HostValueOut {
    let mut out = HostValueAbiOut::default();
    with_scratch(SCRATCH_INITIAL_CAPACITY, |buf| {
        let raw_ref = call(&mut out, buf.as_mut_ptr(), buf.len() as i32);
        if out.kind != HostValueKind::String as i32 {
            return decode_non_string(raw_ref, &out);
        }

        let required = out.string_required_length.max(0) as usize;
        if required > buf.len() {
            // Grow to fit and let the host rewrite the full string once more.
            ensure_capacity(buf, required);
            call(&mut out, buf.as_mut_ptr(), buf.len() as i32);
        }
        let written = (out.string_written_length.max(0) as usize).min(buf.len());
        HostValueOut::String {
            bytes: buf[..written].to_vec(),
            required_len: out.string_required_length.max(0) as usize,
        }
    })
}

/// Like [`string_out_call`] but lends the written bytes to `f` instead of
/// copying them out, so callers that only inspect the string allocate nothing.
///
/// The slice is valid only for the duration of `f`; `f` receives `None` for the
/// absent (negative) sentinel. On a pathological post-grow overflow the slice is
/// clamped to the available capacity rather than erroring (the owned
/// [`crate::Result`] path surfaces that case instead).
pub(crate) fn string_borrow_call<R>(
    call: impl Fn(*mut u8, i32) -> i32,
    f: impl FnOnce(Option<&[u8]>) -> R,
) -> R {
    with_scratch(SCRATCH_INITIAL_CAPACITY, |buf| {
        let required = call(buf.as_mut_ptr(), buf.len() as i32);
        if required < 0 {
            return f(None);
        }
        let required = required as usize;
        if required <= buf.len() {
            return f(Some(&buf[..required]));
        }
        ensure_capacity(buf, required);
        let required = call(buf.as_mut_ptr(), buf.len() as i32);
        if required < 0 {
            return f(None);
        }
        let required = (required as usize).min(buf.len());
        f(Some(&buf[..required]))
    })
}

/// Borrowed dynamic (`any`) host return value. The `Str` slice is valid only for
/// the duration of the closure passed to [`any_borrow_call`].
///
/// Mirrors every [`HostValueOut`] kind so any borrowing caller can decode a
/// dynamic return; today only the `Str`/`Null` arms are inspected (by
/// `get_namespace_uri_with`), so the other payloads are not yet read.
#[allow(dead_code)]
pub(crate) enum AnyBorrow<'a> {
    /// Undefined or any unrecognized kind.
    Undefined,
    /// Null.
    Null,
    /// Boolean.
    Bool(bool),
    /// Number.
    Number(f64),
    /// Borrowed (unvalidated) UTF-8 bytes from the scratch buffer.
    Str(&'a [u8]),
    /// Host-owned reference (arena id).
    ExternRef(i32),
}

/// Like [`any_return`] but lends the string bytes (when present) to `f` instead
/// of copying them, so callers that only inspect the value allocate nothing.
pub(crate) fn any_borrow_call<R>(
    call: impl Fn(*mut HostValueAbiOut, *mut u8, i32) -> i32,
    f: impl FnOnce(AnyBorrow<'_>) -> R,
) -> R {
    let mut out = HostValueAbiOut::default();
    with_scratch(SCRATCH_INITIAL_CAPACITY, |buf| {
        let raw_ref = call(&mut out, buf.as_mut_ptr(), buf.len() as i32);
        if out.kind == HostValueKind::String as i32 {
            let required = out.string_required_length.max(0) as usize;
            if required > buf.len() {
                ensure_capacity(buf, required);
                call(&mut out, buf.as_mut_ptr(), buf.len() as i32);
            }
            let written = (out.string_written_length.max(0) as usize).min(buf.len());
            return f(AnyBorrow::Str(&buf[..written]));
        }
        let borrowed = match out.kind {
            value if value == HostValueKind::Null as i32 => AnyBorrow::Null,
            value if value == HostValueKind::Bool as i32 => AnyBorrow::Bool(out.bool_value != 0),
            value if value == HostValueKind::Number as i32 => AnyBorrow::Number(out.number_value),
            value if value == HostValueKind::ExternRef as i32 => AnyBorrow::ExternRef(raw_ref),
            _ => AnyBorrow::Undefined,
        };
        f(borrowed)
    })
}

macro_rules! any_args {
    ($value:expr) => {
        $value.into_raw_parts()
    };
}

mod ffi {
    use super::HostValueAbiOut;

    #[link(wasm_import_module = "env")]
    extern "C" {
        #[link_name = "__CreateElement"]
        pub fn create_element(tag_ptr: i32, tag_len: i32) -> i32;
        #[link_name = "__CreatePage"]
        pub fn create_page() -> i32;
        #[link_name = "__CreateView"]
        pub fn create_view() -> i32;
        #[link_name = "__CreateScrollView"]
        pub fn create_scroll_view() -> i32;
        #[link_name = "__CreateText"]
        pub fn create_text() -> i32;
        #[link_name = "__CreateImage"]
        pub fn create_image() -> i32;
        #[link_name = "__CreateRawText"]
        pub fn create_raw_text(text_ptr: i32, text_len: i32) -> i32;
        #[link_name = "__CreateNonElement"]
        pub fn create_non_element() -> i32;
        #[link_name = "__CreateWrapperElement"]
        pub fn create_wrapper_element() -> i32;
        #[link_name = "binding__DropElement"]
        pub fn drop_element(element_id: i32);
        #[link_name = "__AppendElement"]
        pub fn append_element(parent: i32, child: i32) -> i32;
        #[link_name = "__RemoveElement"]
        pub fn remove_element(parent: i32, child: i32) -> i32;
        #[link_name = "__InsertElementBefore"]
        pub fn insert_element_before(
            parent: i32,
            child: i32,
            before_kind: i32,
            before_number: f64,
            before_string_ptr: i32,
            before_string_len: i32,
            before_ref: i32,
        ) -> i32;
        #[link_name = "__FirstElement"]
        pub fn first_element(element: i32) -> i32;
        #[link_name = "__LastElement"]
        pub fn last_element(element: i32) -> i32;
        #[link_name = "__NextElement"]
        pub fn next_element(element: i32) -> i32;
        #[link_name = "__ReplaceElement"]
        pub fn replace_element(new_element: i32, old_element: i32);
        #[link_name = "__SwapElement"]
        pub fn swap_element(left: i32, right: i32);
        #[link_name = "__GetParent"]
        pub fn get_parent(element: i32) -> i32;
        #[link_name = "__GetChildren"]
        pub fn get_children(element: i32) -> i32;
        #[link_name = "__ElementIsEqual"]
        pub fn element_is_equal(left: i32, right: i32) -> i32;
        #[link_name = "__GetElementUniqueID"]
        pub fn get_element_unique_id(element: i32) -> i64;
        #[link_name = "__GetTag"]
        pub fn get_tag(element: i32, out_ptr: *mut u8, out_max_len: i32) -> i32;
        #[link_name = "__SetAttribute"]
        pub fn set_attribute(
            element: i32,
            key_kind: i32,
            key_number: f64,
            key_string_ptr: i32,
            key_string_len: i32,
            key_ref: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: i32,
        );
        #[link_name = "__GetAttributes"]
        pub fn get_attributes(element: i32) -> i32;
        #[link_name = "__AddClass"]
        pub fn add_class(element: i32, class_ptr: i32, class_len: i32);
        #[link_name = "__SetClasses"]
        pub fn set_classes(element: i32, classes_ptr: i32, classes_len: i32);
        #[link_name = "__GetClasses"]
        pub fn get_classes(element: i32) -> i32;
        #[link_name = "__AddInlineStyle"]
        pub fn add_inline_style(
            element: i32,
            key_kind: i32,
            key_number: f64,
            key_string_ptr: i32,
            key_string_len: i32,
            key_ref: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: i32,
        );
        #[link_name = "__SetInlineStyles"]
        pub fn set_inline_styles(
            element: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: i32,
        );
        #[link_name = "__GetInlineStyles"]
        pub fn get_inline_styles(element: i32, out_ptr: *mut u8, out_max_len: i32) -> i32;
        #[link_name = "__SetParsedStyles"]
        pub fn set_parsed_styles(
            element: i32,
            styles_ptr: i32,
            styles_len: i32,
            config_kind: i32,
            config_number: f64,
            config_string_ptr: i32,
            config_string_len: i32,
            config_ref: i32,
        );
        #[link_name = "__GetComputedStyles"]
        pub fn get_computed_styles(
            element: i32,
            out: *mut HostValueAbiOut,
            out_string_ptr: *mut u8,
            out_string_max_len: i32,
        ) -> i32;
        #[link_name = "__AddEvent"]
        pub fn add_event(
            element: i32,
            name_ptr: i32,
            name_len: i32,
            type_ptr: i32,
            type_len: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: i32,
        );
        #[link_name = "__SetEvents"]
        pub fn set_events(
            element: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: i32,
        );
        #[link_name = "__GetEvent"]
        pub fn get_event(
            element: i32,
            name_ptr: i32,
            name_len: i32,
            type_ptr: i32,
            type_len: i32,
        ) -> i32;
        #[link_name = "__GetEvents"]
        pub fn get_events(element: i32) -> i32;
        #[link_name = "__SetID"]
        pub fn set_id(
            element: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: i32,
        );
        #[link_name = "__GetID"]
        pub fn get_id(element: i32, out_ptr: *mut u8, out_max_len: i32) -> i32;
        #[link_name = "__AddDataset"]
        pub fn add_dataset(
            element: i32,
            key_ptr: i32,
            key_len: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: i32,
        );
        #[link_name = "__SetDataset"]
        pub fn set_dataset(
            element: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: i32,
        );
        #[link_name = "__GetDataset"]
        pub fn get_dataset(element: i32) -> i32;
        #[link_name = "__FlushElementTree"]
        pub fn flush_element_tree(
            root_kind: i32,
            root_number: f64,
            root_string_ptr: i32,
            root_string_len: i32,
            root_ref: i32,
            options_kind: i32,
            options_number: f64,
            options_string_ptr: i32,
            options_string_len: i32,
            options_ref: i32,
        );
        #[link_name = "_ReportError"]
        pub fn report_error(
            error_kind: i32,
            error_number: f64,
            error_string_ptr: i32,
            error_string_len: i32,
            error_ref: i32,
            info_kind: i32,
            info_number: f64,
            info_string_ptr: i32,
            info_string_len: i32,
            info_ref: i32,
        );
        #[link_name = "__GetDataByKey"]
        pub fn get_data_by_key(
            element: i32,
            key_ptr: i32,
            key_len: i32,
            out: *mut HostValueAbiOut,
            out_string_ptr: *mut u8,
            out_string_max_len: i32,
        ) -> i32;
        #[link_name = "__ReplaceElements"]
        pub fn replace_elements(
            parent: i32,
            new_kind: i32,
            new_number: f64,
            new_string_ptr: i32,
            new_string_len: i32,
            new_ref: i32,
            old_kind: i32,
            old_number: f64,
            old_string_ptr: i32,
            old_string_len: i32,
            old_ref: i32,
        );
        #[link_name = "__QuerySelector"]
        pub fn query_selector(
            element: i32,
            selector_ptr: i32,
            selector_len: i32,
            options_kind: i32,
            options_number: f64,
            options_string_ptr: i32,
            options_string_len: i32,
            options_ref: i32,
        ) -> i32;
        #[link_name = "__QuerySelectorAll"]
        pub fn query_selector_all(
            element: i32,
            selector_ptr: i32,
            selector_len: i32,
            options_kind: i32,
            options_number: f64,
            options_string_ptr: i32,
            options_string_len: i32,
            options_ref: i32,
        ) -> i32;
        #[link_name = "__AddConfig"]
        pub fn add_config(
            element: i32,
            key_ptr: i32,
            key_len: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: i32,
        );
        #[link_name = "__SetConfig"]
        pub fn set_config(
            element: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: i32,
        );
        #[link_name = "__GetConfig"]
        pub fn get_config(element: i32) -> i32;
        #[link_name = "__GetInlineStyle"]
        pub fn get_inline_style(
            element: i32,
            index: i32,
            out: *mut HostValueAbiOut,
            out_string_ptr: *mut u8,
            out_string_max_len: i32,
        ) -> i32;
        #[link_name = "__GetAttributeByName"]
        pub fn get_attribute_by_name(
            element: i32,
            key_ptr: i32,
            key_len: i32,
            out: *mut HostValueAbiOut,
            out_string_ptr: *mut u8,
            out_string_max_len: i32,
        ) -> i32;
        #[link_name = "__GetAttributeNames"]
        pub fn get_attribute_names(element: i32) -> i32;
        #[link_name = "__GetPageElement"]
        pub fn get_page_element() -> i32;
        #[link_name = "__GetElementByUniqueID"]
        pub fn get_element_by_unique_id(unique_id: i64) -> i32;
        #[link_name = "__AddEventListener"]
        pub fn add_event_listener(
            element: i32,
            event_type_ptr: i32,
            event_type_len: i32,
            listener: i32,
            options: i32,
        );
        #[link_name = "__RemoveEventListener"]
        pub fn remove_event_listener(
            element: i32,
            event_type_ptr: i32,
            event_type_len: i32,
            listener: i32,
            options: i32,
        );
        #[link_name = "__CreateEvent"]
        pub fn create_event(
            event_id: i32,
            event_type_ptr: i32,
            event_type_len: i32,
            target: i32,
            options: i32,
        ) -> i32;
        #[link_name = "__DispatchEvent"]
        pub fn dispatch_event(element: i32, event: i32) -> i32;
        #[link_name = "__StopPropagation"]
        pub fn stop_propagation(event: i32);
        #[link_name = "__StopImmediatePropagation"]
        pub fn stop_immediate_propagation(event: i32);
        #[link_name = "__InvokeUIMethod"]
        pub fn invoke_ui_method(
            element: i32,
            method_ptr: i32,
            method_len: i32,
            params: i32,
            callback: i32,
        );
        #[link_name = "__GetComputedStyleByKey"]
        pub fn get_computed_style_by_key(
            element: i32,
            key_ptr: i32,
            key_len: i32,
            out: *mut HostValueAbiOut,
            out_string_ptr: *mut u8,
            out_string_max_len: i32,
        ) -> i32;
        #[link_name = "setTimeout"]
        pub fn set_timeout(callback: i32, delay_ms: i64) -> i64;
        #[link_name = "clearTimeout"]
        pub fn clear_timeout(timer_id: i64);
        #[link_name = "setInterval"]
        pub fn set_interval(callback: i32, delay_ms: i64) -> i64;
        #[link_name = "clearInterval"]
        pub fn clear_interval(timer_id: i64);
    }
}

/// Calls `__CreateElement`. Returns the new element's arena id.
#[inline]
pub fn create_element(tag: &str) -> i32 {
    let (tag_ptr, tag_len) = string_parts(tag);
    unsafe { ffi::create_element(tag_ptr, tag_len) }
}

/// Calls `__CreatePage`.
#[inline]
pub fn create_page() -> i32 {
    unsafe { ffi::create_page() }
}

macro_rules! create_parent_with_info {
    ($name:ident, $ffi_name:ident) => {
        /// Calls the matching create binding. Returns the new element's arena id.
        #[inline]
        pub fn $name() -> i32 {
            unsafe { ffi::$ffi_name() }
        }
    };
}

create_parent_with_info!(create_view, create_view);
create_parent_with_info!(create_scroll_view, create_scroll_view);
create_parent_with_info!(create_text, create_text);
create_parent_with_info!(create_image, create_image);

/// Calls `__CreateRawText`. Returns the new node's arena id.
#[inline]
pub fn create_raw_text(text: &str) -> i32 {
    let (text_ptr, text_len) = string_parts(text);
    unsafe { ffi::create_raw_text(text_ptr, text_len) }
}

/// Calls `__CreateNonElement`.
#[inline]
pub fn create_non_element() -> i32 {
    unsafe { ffi::create_non_element() }
}

/// Calls `__CreateWrapperElement`.
#[inline]
pub fn create_wrapper_element() -> i32 {
    unsafe { ffi::create_wrapper_element() }
}

/// Calls `binding__DropElement`.
#[inline]
pub fn drop_element(element: i32) {
    unsafe { ffi::drop_element(element) }
}

macro_rules! two_ref_return_ref {
    ($name:ident, $ffi_name:ident) => {
        /// Calls the matching two-handle binding. Returns the affected node's id.
        #[inline]
        pub fn $name(left: i32, right: i32) -> i32 {
            unsafe { ffi::$ffi_name(left, right) }
        }
    };
}

two_ref_return_ref!(append_element, append_element);
two_ref_return_ref!(remove_element, remove_element);

/// Calls `__InsertElementBefore`. Returns the inserted child's id.
#[inline]
pub fn insert_element_before(parent: i32, child: i32, before: HostValue<'_>) -> i32 {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(before);
    unsafe {
        ffi::insert_element_before(
            parent, child, kind, number, string_ptr, string_len, ref_value,
        )
    }
}

macro_rules! one_ref_return_ref {
    ($name:ident, $ffi_name:ident) => {
        /// Calls the matching one-handle binding. `None` when the host returns
        /// no node (negative arena id).
        #[inline]
        pub fn $name(element: i32) -> Option<i32> {
            node(unsafe { ffi::$ffi_name(element) })
        }
    };
}

one_ref_return_ref!(first_element, first_element);
one_ref_return_ref!(last_element, last_element);
one_ref_return_ref!(next_element, next_element);
one_ref_return_ref!(get_parent, get_parent);
one_ref_return_ref!(get_children, get_children);
one_ref_return_ref!(get_attributes, get_attributes);
one_ref_return_ref!(get_classes, get_classes);
one_ref_return_ref!(get_events, get_events);
one_ref_return_ref!(get_dataset, get_dataset);
one_ref_return_ref!(get_config, get_config);
one_ref_return_ref!(get_attribute_names, get_attribute_names);

/// Calls `__ReplaceElement`.
pub fn replace_element(new_element: i32, old_element: i32) {
    unsafe { ffi::replace_element(new_element, old_element) }
}

/// Calls `__SwapElement`.
pub fn swap_element(left: i32, right: i32) {
    unsafe { ffi::swap_element(left, right) }
}

/// Calls `__ElementIsEqual`.
pub fn element_is_equal(left: i32, right: i32) -> bool {
    unsafe { ffi::element_is_equal(left, right) != 0 }
}

/// Calls `__GetElementUniqueID`.
pub fn get_element_unique_id(element: i32) -> i64 {
    unsafe { ffi::get_element_unique_id(element) }
}

/// Calls `__GetTag`.
///
/// Thin raw-ABI wrapper: the out-pointer/length describe a guest-owned buffer
/// (today always supplied by [`string_out_call`]); the host only writes within
/// `out_max_len`.
#[inline]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn get_tag(element: i32, out_ptr: *mut u8, out_max_len: i32) -> i32 {
    unsafe { ffi::get_tag(element, out_ptr, out_max_len) }
}

/// Calls `__SetAttribute`.
#[inline]
pub fn set_attribute(element: i32, key: HostValue<'_>, value: HostValue<'_>) {
    let (key_kind, key_number, key_string_ptr, key_string_len, key_ref) = any_args!(key);
    let (value_kind, value_number, value_string_ptr, value_string_len, value_ref) =
        any_args!(value);
    unsafe {
        ffi::set_attribute(
            element,
            key_kind,
            key_number,
            key_string_ptr,
            key_string_len,
            key_ref,
            value_kind,
            value_number,
            value_string_ptr,
            value_string_len,
            value_ref,
        )
    }
}

/// Calls `__AddClass`.
#[inline]
pub fn add_class(element: i32, class_name: &str) {
    let (ptr, len) = string_parts(class_name);
    unsafe { ffi::add_class(element, ptr, len) }
}

/// Calls `__SetClasses`.
#[inline]
pub fn set_classes(element: i32, classes: &str) {
    let (ptr, len) = string_parts(classes);
    unsafe { ffi::set_classes(element, ptr, len) }
}

/// Calls `__AddInlineStyle`.
#[inline]
pub fn add_inline_style(element: i32, key: HostValue<'_>, value: HostValue<'_>) {
    let (key_kind, key_number, key_string_ptr, key_string_len, key_ref) = any_args!(key);
    let (value_kind, value_number, value_string_ptr, value_string_len, value_ref) =
        any_args!(value);
    unsafe {
        ffi::add_inline_style(
            element,
            key_kind,
            key_number,
            key_string_ptr,
            key_string_len,
            key_ref,
            value_kind,
            value_number,
            value_string_ptr,
            value_string_len,
            value_ref,
        )
    }
}

/// Calls `__SetInlineStyles`.
#[inline]
pub fn set_inline_styles(element: i32, value: HostValue<'_>) {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe { ffi::set_inline_styles(element, kind, number, string_ptr, string_len, ref_value) }
}

/// Calls `__GetInlineStyles`.
///
/// Thin raw-ABI wrapper: the out-pointer/length describe a guest-owned buffer;
/// the host only writes within `out_max_len`.
#[inline]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn get_inline_styles(element: i32, out_ptr: *mut u8, out_max_len: i32) -> i32 {
    unsafe { ffi::get_inline_styles(element, out_ptr, out_max_len) }
}

/// Calls `__SetParsedStyles`.
pub fn set_parsed_styles(element: i32, styles: &str, config: HostValue<'_>) {
    let (styles_ptr, styles_len) = string_parts(styles);
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(config);
    unsafe {
        ffi::set_parsed_styles(
            element, styles_ptr, styles_len, kind, number, string_ptr, string_len, ref_value,
        )
    }
}

/// Calls `__GetComputedStyles`.
pub fn get_computed_styles(element: i32) -> HostValueOut {
    any_return(|out, string_ptr, string_len| unsafe {
        ffi::get_computed_styles(element, out, string_ptr, string_len)
    })
}

/// Calls `__AddEvent`.
#[inline]
pub fn add_event(element: i32, name: &str, event_type: &str, value: HostValue<'_>) {
    let (name_ptr, name_len) = string_parts(name);
    let (type_ptr, type_len) = string_parts(event_type);
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe {
        ffi::add_event(
            element, name_ptr, name_len, type_ptr, type_len, kind, number, string_ptr, string_len,
            ref_value,
        )
    }
}

/// Calls `__SetEvents`.
#[inline]
pub fn set_events(element: i32, value: HostValue<'_>) {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe { ffi::set_events(element, kind, number, string_ptr, string_len, ref_value) }
}

/// Calls `__GetEvent`. `None` when the host returns no event.
pub fn get_event(element: i32, name: &str, event_type: &str) -> Option<i32> {
    let (name_ptr, name_len) = string_parts(name);
    let (type_ptr, type_len) = string_parts(event_type);
    node(unsafe { ffi::get_event(element, name_ptr, name_len, type_ptr, type_len) })
}

/// Calls `__SetID`.
#[inline]
pub fn set_id(element: i32, value: HostValue<'_>) {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe { ffi::set_id(element, kind, number, string_ptr, string_len, ref_value) }
}

/// Calls `__GetID`.
///
/// Thin raw-ABI wrapper: the out-pointer/length describe a guest-owned buffer;
/// the host only writes within `out_max_len`.
#[inline]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn get_id(element: i32, out_ptr: *mut u8, out_max_len: i32) -> i32 {
    unsafe { ffi::get_id(element, out_ptr, out_max_len) }
}

/// Calls `__AddDataset`.
#[inline]
pub fn add_dataset(element: i32, key: &str, value: HostValue<'_>) {
    let (key_ptr, key_len) = string_parts(key);
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe {
        ffi::add_dataset(
            element, key_ptr, key_len, kind, number, string_ptr, string_len, ref_value,
        )
    }
}

/// Calls `__SetDataset`.
#[inline]
pub fn set_dataset(element: i32, value: HostValue<'_>) {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe { ffi::set_dataset(element, kind, number, string_ptr, string_len, ref_value) }
}

/// Calls `__FlushElementTree`.
pub fn flush_element_tree(root: HostValue<'_>, options: HostValue<'_>) {
    let (root_kind, root_number, root_string_ptr, root_string_len, root_ref) = any_args!(root);
    let (options_kind, options_number, options_string_ptr, options_string_len, options_ref) =
        any_args!(options);
    unsafe {
        ffi::flush_element_tree(
            root_kind,
            root_number,
            root_string_ptr,
            root_string_len,
            root_ref,
            options_kind,
            options_number,
            options_string_ptr,
            options_string_len,
            options_ref,
        )
    }
}

/// Calls `_ReportError`.
pub fn report_error(error: HostValue<'_>, info: HostValue<'_>) {
    let (error_kind, error_number, error_string_ptr, error_string_len, error_ref) =
        any_args!(error);
    let (info_kind, info_number, info_string_ptr, info_string_len, info_ref) = any_args!(info);
    unsafe {
        ffi::report_error(
            error_kind,
            error_number,
            error_string_ptr,
            error_string_len,
            error_ref,
            info_kind,
            info_number,
            info_string_ptr,
            info_string_len,
            info_ref,
        )
    }
}

/// Calls `__GetDataByKey`.
pub fn get_data_by_key(element: i32, key: &str) -> HostValueOut {
    let (key_ptr, key_len) = string_parts(key);
    any_return(|out, string_ptr, string_len| unsafe {
        ffi::get_data_by_key(element, key_ptr, key_len, out, string_ptr, string_len)
    })
}

/// Calls `__ReplaceElements`.
pub fn replace_elements(parent: i32, new_elements: HostValue<'_>, old_elements: HostValue<'_>) {
    let (new_kind, new_number, new_string_ptr, new_string_len, new_ref) = any_args!(new_elements);
    let (old_kind, old_number, old_string_ptr, old_string_len, old_ref) = any_args!(old_elements);
    unsafe {
        ffi::replace_elements(
            parent,
            new_kind,
            new_number,
            new_string_ptr,
            new_string_len,
            new_ref,
            old_kind,
            old_number,
            old_string_ptr,
            old_string_len,
            old_ref,
        )
    }
}

/// Calls `__QuerySelector`. `None` when no element matches.
pub fn query_selector(element: i32, selector: &str, options: HostValue<'_>) -> Option<i32> {
    let (selector_ptr, selector_len) = string_parts(selector);
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(options);
    node(unsafe {
        ffi::query_selector(
            element,
            selector_ptr,
            selector_len,
            kind,
            number,
            string_ptr,
            string_len,
            ref_value,
        )
    })
}

/// Calls `__QuerySelectorAll`. `None` when the host returns no result list.
pub fn query_selector_all(element: i32, selector: &str, options: HostValue<'_>) -> Option<i32> {
    let (selector_ptr, selector_len) = string_parts(selector);
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(options);
    node(unsafe {
        ffi::query_selector_all(
            element,
            selector_ptr,
            selector_len,
            kind,
            number,
            string_ptr,
            string_len,
            ref_value,
        )
    })
}

/// Calls `__AddConfig`.
pub fn add_config(element: i32, key: &str, value: HostValue<'_>) {
    let (key_ptr, key_len) = string_parts(key);
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe {
        ffi::add_config(
            element, key_ptr, key_len, kind, number, string_ptr, string_len, ref_value,
        )
    }
}

/// Calls `__SetConfig`.
pub fn set_config(element: i32, value: HostValue<'_>) {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe { ffi::set_config(element, kind, number, string_ptr, string_len, ref_value) }
}

/// Calls `__GetInlineStyle`.
pub fn get_inline_style(element: i32, index: i32) -> HostValueOut {
    any_return(|out, string_ptr, string_len| unsafe {
        ffi::get_inline_style(element, index, out, string_ptr, string_len)
    })
}

/// Calls `__GetAttributeByName`.
pub fn get_attribute_by_name(element: i32, key: &str) -> HostValueOut {
    let (key_ptr, key_len) = string_parts(key);
    any_return(|out, string_ptr, string_len| unsafe {
        ffi::get_attribute_by_name(element, key_ptr, key_len, out, string_ptr, string_len)
    })
}

/// Borrowing variant of [`get_attribute_by_name`] that lends the value to `f`
/// without allocating an owned string.
#[inline]
pub(crate) fn get_attribute_by_name_borrow<R>(
    element: i32,
    key: &str,
    f: impl FnOnce(AnyBorrow<'_>) -> R,
) -> R {
    let (key_ptr, key_len) = string_parts(key);
    any_borrow_call(
        |out, string_ptr, string_len| unsafe {
            ffi::get_attribute_by_name(element, key_ptr, key_len, out, string_ptr, string_len)
        },
        f,
    )
}

/// Calls `__GetPageElement`. `None` when there is no page element.
pub fn get_page_element() -> Option<i32> {
    node(unsafe { ffi::get_page_element() })
}

/// Calls `__GetElementByUniqueID`. `None` when no element has that id.
pub fn get_element_by_unique_id(unique_id: i64) -> Option<i32> {
    node(unsafe { ffi::get_element_by_unique_id(unique_id) })
}

/// Calls `__AddEventListener`.
pub fn add_event_listener(element: i32, event_type: &str, listener: i32, options: i32) {
    let (event_type_ptr, event_type_len) = string_parts(event_type);
    unsafe { ffi::add_event_listener(element, event_type_ptr, event_type_len, listener, options) }
}

/// Calls `__RemoveEventListener`.
pub fn remove_event_listener(element: i32, event_type: &str, listener: i32, options: i32) {
    let (event_type_ptr, event_type_len) = string_parts(event_type);
    unsafe {
        ffi::remove_event_listener(element, event_type_ptr, event_type_len, listener, options)
    }
}

/// Calls `__CreateEvent`. `None` when creation fails.
pub fn create_event(event_id: i32, event_type: &str, target: i32, options: i32) -> Option<i32> {
    let (event_type_ptr, event_type_len) = string_parts(event_type);
    node(unsafe { ffi::create_event(event_id, event_type_ptr, event_type_len, target, options) })
}

/// Calls `__DispatchEvent`.
pub fn dispatch_event(element: i32, event: i32) -> bool {
    unsafe { ffi::dispatch_event(element, event) != 0 }
}

/// Calls `__StopPropagation`.
pub fn stop_propagation(event: i32) {
    unsafe { ffi::stop_propagation(event) }
}

/// Calls `__StopImmediatePropagation`.
pub fn stop_immediate_propagation(event: i32) {
    unsafe { ffi::stop_immediate_propagation(event) }
}

/// Calls `__InvokeUIMethod`.
pub fn invoke_ui_method(element: i32, method: &str, params: i32, callback: i32) {
    let (method_ptr, method_len) = string_parts(method);
    unsafe { ffi::invoke_ui_method(element, method_ptr, method_len, params, callback) }
}

/// Calls `__GetComputedStyleByKey`.
pub fn get_computed_style_by_key(element: i32, key: &str) -> HostValueOut {
    let (key_ptr, key_len) = string_parts(key);
    any_return(|out, string_ptr, string_len| unsafe {
        ffi::get_computed_style_by_key(element, key_ptr, key_len, out, string_ptr, string_len)
    })
}

/// Calls `setTimeout`.
pub fn set_timeout(callback: i32, delay_ms: i64) -> i64 {
    unsafe { ffi::set_timeout(callback, delay_ms) }
}

/// Calls `clearTimeout`.
pub fn clear_timeout(timer_id: i64) {
    unsafe { ffi::clear_timeout(timer_id) }
}

/// Calls `setInterval`.
pub fn set_interval(callback: i32, delay_ms: i64) -> i64 {
    unsafe { ffi::set_interval(callback, delay_ms) }
}

/// Calls `clearInterval`.
pub fn clear_interval(timer_id: i64) {
    unsafe { ffi::clear_interval(timer_id) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake host that writes `payload` into the buffer when it fits and
    /// reports the payload length as the required length.
    fn writer(payload: &'static [u8]) -> impl Fn(*mut u8, i32) -> i32 {
        move |ptr, max| {
            let n = payload.len();
            if (max as usize) >= n {
                // SAFETY: test-only; `ptr` points at `max` writable bytes.
                unsafe { std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr, n) };
            }
            n as i32
        }
    }

    #[test]
    fn node_maps_negative_to_none() {
        assert_eq!(node(0), Some(0));
        assert_eq!(node(7), Some(7));
        assert_eq!(node(-1), None);
        assert_eq!(node(NULL_NODE), None);
    }

    #[test]
    fn string_out_is_right_sized() {
        match string_out_call(writer(b"div")) {
            StringOut::Bytes(bytes) => {
                assert_eq!(bytes, b"div");
                // Right-sized result, not backed by the scratch allocation.
                assert!(bytes.capacity() < SCRATCH_INITIAL_CAPACITY);
            }
            _ => panic!("expected bytes"),
        }
    }

    #[test]
    fn string_out_absent_on_negative() {
        assert!(matches!(
            string_out_call(|_ptr, _max| -1),
            StringOut::Absent
        ));
    }

    #[test]
    fn string_out_grows_and_retries() {
        let payload = vec![b'x'; SCRATCH_INITIAL_CAPACITY + 100];
        let expected_len = payload.len();
        let call = move |ptr: *mut u8, max: i32| {
            let n = payload.len();
            if (max as usize) >= n {
                unsafe { std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr, n) };
            }
            n as i32
        };
        match string_out_call(call) {
            StringOut::Bytes(bytes) => assert_eq!(bytes.len(), expected_len),
            _ => panic!("expected full payload after retry"),
        }
    }

    #[test]
    fn string_out_overflows_when_host_misreports() {
        // Always demands more than offered, even after growing once.
        match string_out_call(|_ptr, max| max.saturating_add(1024)) {
            StringOut::Overflow { .. } => {}
            _ => panic!("expected overflow"),
        }
    }

    #[test]
    fn scratch_is_reused_across_calls() {
        let _ = string_out_call(writer(b"first"));
        let cap_before = SCRATCH.with(|cell| cell.borrow().capacity());
        let _ = string_out_call(writer(b"second"));
        let cap_after = SCRATCH.with(|cell| cell.borrow().capacity());
        assert_eq!(cap_before, cap_after);
        assert!(cap_before >= SCRATCH_INITIAL_CAPACITY);
    }

    #[test]
    fn reentrant_use_falls_back_without_panic() {
        SCRATCH.with(|cell| {
            let _guard = cell.borrow_mut();
            match string_out_call(writer(b"ok")) {
                StringOut::Bytes(bytes) => assert_eq!(bytes, b"ok"),
                _ => panic!("expected bytes via fallback"),
            }
        });
    }

    #[test]
    fn any_return_non_string_skips_buffer() {
        let null = |out: *mut HostValueAbiOut, _ptr: *mut u8, _max: i32| {
            unsafe { (*out).kind = HostValueKind::Null as i32 };
            NULL_NODE
        };
        assert_eq!(any_return(null), HostValueOut::Null);

        let number = |out: *mut HostValueAbiOut, _ptr: *mut u8, _max: i32| {
            unsafe {
                (*out).kind = HostValueKind::Number as i32;
                (*out).number_value = 42.5;
            }
            NULL_NODE
        };
        assert_eq!(any_return(number), HostValueOut::Number(42.5));
    }

    #[test]
    fn any_return_extern_ref_carries_id() {
        let call = |out: *mut HostValueAbiOut, _ptr: *mut u8, _max: i32| {
            unsafe { (*out).kind = HostValueKind::ExternRef as i32 };
            7
        };
        assert_eq!(any_return(call), HostValueOut::ExternRef(7));
    }

    #[test]
    fn any_return_string_is_right_sized() {
        let payload = b"hello";
        let call = move |out: *mut HostValueAbiOut, ptr: *mut u8, max: i32| {
            let n = payload.len();
            unsafe {
                (*out).kind = HostValueKind::String as i32;
                (*out).string_required_length = n as i32;
                if (max as usize) >= n {
                    std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr, n);
                    (*out).string_written_length = n as i32;
                }
            }
            NULL_NODE
        };
        match any_return(call) {
            HostValueOut::String {
                bytes,
                required_len,
            } => {
                assert_eq!(bytes, b"hello");
                assert_eq!(required_len, 5);
                assert!(bytes.capacity() < SCRATCH_INITIAL_CAPACITY);
            }
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[test]
    fn string_borrow_lends_without_owning() {
        let owned = string_borrow_call(writer(b"svg"), |bytes| bytes.map(<[u8]>::to_vec));
        assert_eq!(owned, Some(b"svg".to_vec()));
    }

    #[test]
    fn string_borrow_absent_on_negative() {
        assert!(string_borrow_call(|_ptr, _max| -1, |bytes| bytes.is_none()));
    }

    #[test]
    fn any_borrow_string_and_null() {
        let payload = b"http://www.w3.org/2000/svg";
        let str_call = move |out: *mut HostValueAbiOut, ptr: *mut u8, max: i32| {
            let n = payload.len();
            unsafe {
                (*out).kind = HostValueKind::String as i32;
                (*out).string_required_length = n as i32;
                if (max as usize) >= n {
                    std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr, n);
                    (*out).string_written_length = n as i32;
                }
            }
            NULL_NODE
        };
        assert!(any_borrow_call(str_call, |value| match value {
            AnyBorrow::Str(bytes) => bytes == b"http://www.w3.org/2000/svg",
            _ => false,
        }));

        let null_call = |out: *mut HostValueAbiOut, _ptr: *mut u8, _max: i32| {
            unsafe { (*out).kind = HostValueKind::Null as i32 };
            NULL_NODE
        };
        assert!(any_borrow_call(null_call, |value| matches!(
            value,
            AnyBorrow::Null
        )));
    }
}
