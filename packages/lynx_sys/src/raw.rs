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

/// Host-owned value reference.
///
/// Rust stable currently does not expose a first-class `externref` FFI type.
/// Keep the representation isolated here so the public binding surface carries
/// `ExternRef` directly while the ABI carrier can be updated in one place.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ExternRef(u32);

impl ExternRef {
    /// Creates a wrapper from the raw ABI carrier.
    #[inline(always)]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns a null host reference.
    #[inline(always)]
    pub const fn null() -> Self {
        Self(0)
    }

    /// Returns the raw ABI carrier.
    #[inline(always)]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Returns true when this reference is null.
    #[inline(always)]
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }
}

impl std::fmt::Debug for ExternRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_null() {
            f.write_str("ExternRef(null)")
        } else {
            f.debug_tuple("ExternRef").field(&self.0).finish()
        }
    }
}

/// Dynamic host value used for copied ABI values and externrefs.
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
    /// Host-owned reference.
    ExternRef(ExternRef),
}

impl HostValue<'_> {
    #[inline(always)]
    fn into_raw_parts(self) -> (i32, f64, i32, i32, ExternRef) {
        match self {
            Self::Undefined => (
                HostValueKind::Undefined as i32,
                0.0,
                0,
                0,
                ExternRef::null(),
            ),
            Self::Null => (HostValueKind::Null as i32, 0.0, 0, 0, ExternRef::null()),
            Self::Bool(value) => (
                HostValueKind::Bool as i32,
                if value { 1.0 } else { 0.0 },
                0,
                0,
                ExternRef::null(),
            ),
            Self::Number(value) => (HostValueKind::Number as i32, value, 0, 0, ExternRef::null()),
            Self::String(value) => (
                HostValueKind::String as i32,
                0.0,
                value.as_ptr() as i32,
                value.len() as i32,
                ExternRef::null(),
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
    /// Host-owned reference.
    ExternRef(ExternRef),
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
fn decode_non_string(raw_ref: ExternRef, out: &HostValueAbiOut) -> HostValueOut {
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
fn any_return(call: impl Fn(*mut HostValueAbiOut, *mut u8, i32) -> ExternRef) -> HostValueOut {
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
    /// Host-owned reference.
    ExternRef(ExternRef),
}

/// Like [`any_return`] but lends the string bytes (when present) to `f` instead
/// of copying them, so callers that only inspect the value allocate nothing.
pub(crate) fn any_borrow_call<R>(
    call: impl Fn(*mut HostValueAbiOut, *mut u8, i32) -> ExternRef,
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

#[cfg(target_arch = "wasm32")]
mod ffi {
    use super::{ExternRef, HostValueAbiOut};

    #[link(wasm_import_module = "env")]
    extern "C" {
        #[link_name = "__CreateElement"]
        pub fn create_element(tag_ptr: i32, tag_len: i32) -> u32;
        #[link_name = "__CreatePage"]
        pub fn create_page() -> u32;
        #[link_name = "__CreateView"]
        pub fn create_view() -> u32;
        #[link_name = "__CreateScrollView"]
        pub fn create_scroll_view() -> u32;
        #[link_name = "__CreateText"]
        pub fn create_text() -> u32;
        #[link_name = "__CreateImage"]
        pub fn create_image() -> u32;
        #[link_name = "__CreateRawText"]
        pub fn create_raw_text(text_ptr: i32, text_len: i32) -> u32;
        #[link_name = "__CreateNonElement"]
        pub fn create_non_element() -> u32;
        #[link_name = "__CreateWrapperElement"]
        pub fn create_wrapper_element() -> u32;
        #[link_name = "binding__DropElement"]
        pub fn drop_element(element_id: u32);
        #[link_name = "__AppendElement"]
        pub fn append_element(parent: ExternRef, child: ExternRef) -> ExternRef;
        #[link_name = "__RemoveElement"]
        pub fn remove_element(parent: ExternRef, child: ExternRef) -> ExternRef;
        #[link_name = "__InsertElementBefore"]
        pub fn insert_element_before(
            parent: ExternRef,
            child: ExternRef,
            before_kind: i32,
            before_number: f64,
            before_string_ptr: i32,
            before_string_len: i32,
            before_ref: ExternRef,
        ) -> ExternRef;
        #[link_name = "__FirstElement"]
        pub fn first_element(element: ExternRef) -> ExternRef;
        #[link_name = "__LastElement"]
        pub fn last_element(element: ExternRef) -> ExternRef;
        #[link_name = "__NextElement"]
        pub fn next_element(element: ExternRef) -> ExternRef;
        #[link_name = "__ReplaceElement"]
        pub fn replace_element(new_element: ExternRef, old_element: ExternRef);
        #[link_name = "__SwapElement"]
        pub fn swap_element(left: ExternRef, right: ExternRef);
        #[link_name = "__GetParent"]
        pub fn get_parent(element: ExternRef) -> ExternRef;
        #[link_name = "__GetChildren"]
        pub fn get_children(element: ExternRef) -> ExternRef;
        #[link_name = "__ElementIsEqual"]
        pub fn element_is_equal(left: ExternRef, right: ExternRef) -> i32;
        #[link_name = "__GetElementUniqueID"]
        pub fn get_element_unique_id(element: ExternRef) -> i64;
        #[link_name = "__GetTag"]
        pub fn get_tag(element: ExternRef, out_ptr: *mut u8, out_max_len: i32) -> i32;
        #[link_name = "__SetAttribute"]
        pub fn set_attribute(
            element: ExternRef,
            key_kind: i32,
            key_number: f64,
            key_string_ptr: i32,
            key_string_len: i32,
            key_ref: ExternRef,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: ExternRef,
        );
        #[link_name = "__GetAttributes"]
        pub fn get_attributes(element: ExternRef) -> ExternRef;
        #[link_name = "__AddClass"]
        pub fn add_class(element: ExternRef, class_ptr: i32, class_len: i32);
        #[link_name = "__SetClasses"]
        pub fn set_classes(element: ExternRef, classes_ptr: i32, classes_len: i32);
        #[link_name = "__GetClasses"]
        pub fn get_classes(element: ExternRef) -> ExternRef;
        #[link_name = "__AddInlineStyle"]
        pub fn add_inline_style(
            element: ExternRef,
            key_kind: i32,
            key_number: f64,
            key_string_ptr: i32,
            key_string_len: i32,
            key_ref: ExternRef,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: ExternRef,
        );
        #[link_name = "__SetInlineStyles"]
        pub fn set_inline_styles(
            element: ExternRef,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: ExternRef,
        );
        #[link_name = "__GetInlineStyles"]
        pub fn get_inline_styles(element: ExternRef, out_ptr: *mut u8, out_max_len: i32) -> i32;
        #[link_name = "__SetParsedStyles"]
        pub fn set_parsed_styles(
            element: ExternRef,
            styles_ptr: i32,
            styles_len: i32,
            config_kind: i32,
            config_number: f64,
            config_string_ptr: i32,
            config_string_len: i32,
            config_ref: ExternRef,
        );
        #[link_name = "__GetComputedStyles"]
        pub fn get_computed_styles(
            element: ExternRef,
            out: *mut HostValueAbiOut,
            out_string_ptr: *mut u8,
            out_string_max_len: i32,
        ) -> ExternRef;
        #[link_name = "__AddEvent"]
        pub fn add_event(
            element: ExternRef,
            name_ptr: i32,
            name_len: i32,
            type_ptr: i32,
            type_len: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: ExternRef,
        );
        #[link_name = "__SetEvents"]
        pub fn set_events(
            element: ExternRef,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: ExternRef,
        );
        #[link_name = "__GetEvent"]
        pub fn get_event(
            element: ExternRef,
            name_ptr: i32,
            name_len: i32,
            type_ptr: i32,
            type_len: i32,
        ) -> ExternRef;
        #[link_name = "__GetEvents"]
        pub fn get_events(element: ExternRef) -> ExternRef;
        #[link_name = "__SetID"]
        pub fn set_id(
            element: ExternRef,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: ExternRef,
        );
        #[link_name = "__GetID"]
        pub fn get_id(element: ExternRef, out_ptr: *mut u8, out_max_len: i32) -> i32;
        #[link_name = "__AddDataset"]
        pub fn add_dataset(
            element: ExternRef,
            key_ptr: i32,
            key_len: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: ExternRef,
        );
        #[link_name = "__SetDataset"]
        pub fn set_dataset(
            element: ExternRef,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: ExternRef,
        );
        #[link_name = "__GetDataset"]
        pub fn get_dataset(element: ExternRef) -> ExternRef;
        #[link_name = "__FlushElementTree"]
        pub fn flush_element_tree(
            root_kind: i32,
            root_number: f64,
            root_string_ptr: i32,
            root_string_len: i32,
            root_ref: ExternRef,
            options_kind: i32,
            options_number: f64,
            options_string_ptr: i32,
            options_string_len: i32,
            options_ref: ExternRef,
        );
        #[link_name = "_ReportError"]
        pub fn report_error(
            error_kind: i32,
            error_number: f64,
            error_string_ptr: i32,
            error_string_len: i32,
            error_ref: ExternRef,
            info_kind: i32,
            info_number: f64,
            info_string_ptr: i32,
            info_string_len: i32,
            info_ref: ExternRef,
        );
        #[link_name = "__GetDataByKey"]
        pub fn get_data_by_key(
            element: ExternRef,
            key_ptr: i32,
            key_len: i32,
            out: *mut HostValueAbiOut,
            out_string_ptr: *mut u8,
            out_string_max_len: i32,
        ) -> ExternRef;
        #[link_name = "__ReplaceElements"]
        pub fn replace_elements(
            parent: ExternRef,
            new_kind: i32,
            new_number: f64,
            new_string_ptr: i32,
            new_string_len: i32,
            new_ref: ExternRef,
            old_kind: i32,
            old_number: f64,
            old_string_ptr: i32,
            old_string_len: i32,
            old_ref: ExternRef,
        );
        #[link_name = "__QuerySelector"]
        pub fn query_selector(
            element: ExternRef,
            selector_ptr: i32,
            selector_len: i32,
            options_kind: i32,
            options_number: f64,
            options_string_ptr: i32,
            options_string_len: i32,
            options_ref: ExternRef,
        ) -> ExternRef;
        #[link_name = "__QuerySelectorAll"]
        pub fn query_selector_all(
            element: ExternRef,
            selector_ptr: i32,
            selector_len: i32,
            options_kind: i32,
            options_number: f64,
            options_string_ptr: i32,
            options_string_len: i32,
            options_ref: ExternRef,
        ) -> ExternRef;
        #[link_name = "__AddConfig"]
        pub fn add_config(
            element: ExternRef,
            key_ptr: i32,
            key_len: i32,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: ExternRef,
        );
        #[link_name = "__SetConfig"]
        pub fn set_config(
            element: ExternRef,
            value_kind: i32,
            value_number: f64,
            value_string_ptr: i32,
            value_string_len: i32,
            value_ref: ExternRef,
        );
        #[link_name = "__GetConfig"]
        pub fn get_config(element: ExternRef) -> ExternRef;
        #[link_name = "__GetInlineStyle"]
        pub fn get_inline_style(
            element: ExternRef,
            index: i32,
            out: *mut HostValueAbiOut,
            out_string_ptr: *mut u8,
            out_string_max_len: i32,
        ) -> ExternRef;
        #[link_name = "__GetAttributeByName"]
        pub fn get_attribute_by_name(
            element: ExternRef,
            key_ptr: i32,
            key_len: i32,
            out: *mut HostValueAbiOut,
            out_string_ptr: *mut u8,
            out_string_max_len: i32,
        ) -> ExternRef;
        #[link_name = "__GetAttributeNames"]
        pub fn get_attribute_names(element: ExternRef) -> ExternRef;
        #[link_name = "__GetPageElement"]
        pub fn get_page_element() -> ExternRef;
        #[link_name = "__GetElementByUniqueID"]
        pub fn get_element_by_unique_id(unique_id: i64) -> ExternRef;
        #[link_name = "__AddEventListener"]
        pub fn add_event_listener(
            element: ExternRef,
            event_type_ptr: i32,
            event_type_len: i32,
            listener: ExternRef,
            options: ExternRef,
        );
        #[link_name = "__RemoveEventListener"]
        pub fn remove_event_listener(
            element: ExternRef,
            event_type_ptr: i32,
            event_type_len: i32,
            listener: ExternRef,
            options: ExternRef,
        );
        #[link_name = "__CreateEvent"]
        pub fn create_event(
            event_id: i32,
            event_type_ptr: i32,
            event_type_len: i32,
            target: ExternRef,
            options: ExternRef,
        ) -> ExternRef;
        #[link_name = "__DispatchEvent"]
        pub fn dispatch_event(element: ExternRef, event: ExternRef) -> i32;
        #[link_name = "__StopPropagation"]
        pub fn stop_propagation(event: ExternRef);
        #[link_name = "__StopImmediatePropagation"]
        pub fn stop_immediate_propagation(event: ExternRef);
        #[link_name = "__InvokeUIMethod"]
        pub fn invoke_ui_method(
            element: ExternRef,
            method_ptr: i32,
            method_len: i32,
            params: ExternRef,
            callback: ExternRef,
        );
        #[link_name = "__GetComputedStyleByKey"]
        pub fn get_computed_style_by_key(
            element: ExternRef,
            key_ptr: i32,
            key_len: i32,
            out: *mut HostValueAbiOut,
            out_string_ptr: *mut u8,
            out_string_max_len: i32,
        ) -> ExternRef;
        #[link_name = "setTimeout"]
        pub fn set_timeout(callback: ExternRef, delay_ms: i64) -> i64;
        #[link_name = "clearTimeout"]
        pub fn clear_timeout(timer_id: i64);
        #[link_name = "setInterval"]
        pub fn set_interval(callback: ExternRef, delay_ms: i64) -> i64;
        #[link_name = "clearInterval"]
        pub fn clear_interval(timer_id: i64);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
mod ffi {
    use super::{ExternRef, HostValueAbiOut};

    pub unsafe fn create_element(_: i32, _: i32) -> u32 {
        0
    }
    pub unsafe fn create_page() -> u32 {
        0
    }
    pub unsafe fn create_view() -> u32 {
        0
    }
    pub unsafe fn create_scroll_view() -> u32 {
        0
    }
    pub unsafe fn create_text() -> u32 {
        0
    }
    pub unsafe fn create_image() -> u32 {
        0
    }
    pub unsafe fn create_raw_text(_: i32, _: i32) -> u32 {
        0
    }
    pub unsafe fn create_non_element() -> u32 {
        0
    }
    pub unsafe fn create_wrapper_element() -> u32 {
        0
    }
    pub unsafe fn drop_element(_: u32) {}
    pub unsafe fn append_element(_: ExternRef, child: ExternRef) -> ExternRef {
        child
    }
    pub unsafe fn remove_element(_: ExternRef, child: ExternRef) -> ExternRef {
        child
    }
    pub unsafe fn insert_element_before(
        _: ExternRef,
        child: ExternRef,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
    ) -> ExternRef {
        child
    }
    pub unsafe fn first_element(_: ExternRef) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn last_element(_: ExternRef) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn next_element(_: ExternRef) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn replace_element(_: ExternRef, _: ExternRef) {}
    pub unsafe fn swap_element(_: ExternRef, _: ExternRef) {}
    pub unsafe fn get_parent(_: ExternRef) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn get_children(_: ExternRef) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn element_is_equal(left: ExternRef, right: ExternRef) -> i32 {
        (left == right) as i32
    }
    pub unsafe fn get_element_unique_id(element: ExternRef) -> i64 {
        element.raw() as i64
    }
    pub unsafe fn get_tag(_: ExternRef, _: *mut u8, _: i32) -> i32 {
        -1
    }
    pub unsafe fn set_attribute(
        _: ExternRef,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
    ) {
    }
    pub unsafe fn get_attributes(_: ExternRef) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn add_class(_: ExternRef, _: i32, _: i32) {}
    pub unsafe fn set_classes(_: ExternRef, _: i32, _: i32) {}
    pub unsafe fn get_classes(_: ExternRef) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn add_inline_style(
        _: ExternRef,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
    ) {
    }
    pub unsafe fn set_inline_styles(_: ExternRef, _: i32, _: f64, _: i32, _: i32, _: ExternRef) {}
    pub unsafe fn get_inline_styles(_: ExternRef, _: *mut u8, _: i32) -> i32 {
        -1
    }
    pub unsafe fn set_parsed_styles(
        _: ExternRef,
        _: i32,
        _: i32,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
    ) {
    }
    pub unsafe fn get_computed_styles(
        _: ExternRef,
        _: *mut HostValueAbiOut,
        _: *mut u8,
        _: i32,
    ) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn add_event(
        _: ExternRef,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
    ) {
    }
    pub unsafe fn set_events(_: ExternRef, _: i32, _: f64, _: i32, _: i32, _: ExternRef) {}
    pub unsafe fn get_event(_: ExternRef, _: i32, _: i32, _: i32, _: i32) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn get_events(_: ExternRef) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn set_id(_: ExternRef, _: i32, _: f64, _: i32, _: i32, _: ExternRef) {}
    pub unsafe fn get_id(_: ExternRef, _: *mut u8, _: i32) -> i32 {
        -1
    }
    pub unsafe fn add_dataset(
        _: ExternRef,
        _: i32,
        _: i32,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
    ) {
    }
    pub unsafe fn set_dataset(_: ExternRef, _: i32, _: f64, _: i32, _: i32, _: ExternRef) {}
    pub unsafe fn get_dataset(_: ExternRef) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn flush_element_tree(
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
    ) {
    }
    pub unsafe fn report_error(
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
    ) {
    }
    pub unsafe fn get_data_by_key(
        _: ExternRef,
        _: i32,
        _: i32,
        _: *mut HostValueAbiOut,
        _: *mut u8,
        _: i32,
    ) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn replace_elements(
        _: ExternRef,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
    ) {
    }
    pub unsafe fn query_selector(
        _: ExternRef,
        _: i32,
        _: i32,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
    ) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn query_selector_all(
        _: ExternRef,
        _: i32,
        _: i32,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
    ) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn add_config(
        _: ExternRef,
        _: i32,
        _: i32,
        _: i32,
        _: f64,
        _: i32,
        _: i32,
        _: ExternRef,
    ) {
    }
    pub unsafe fn set_config(_: ExternRef, _: i32, _: f64, _: i32, _: i32, _: ExternRef) {}
    pub unsafe fn get_config(_: ExternRef) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn get_inline_style(
        _: ExternRef,
        _: i32,
        _: *mut HostValueAbiOut,
        _: *mut u8,
        _: i32,
    ) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn get_attribute_by_name(
        _: ExternRef,
        _: i32,
        _: i32,
        _: *mut HostValueAbiOut,
        _: *mut u8,
        _: i32,
    ) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn get_attribute_names(_: ExternRef) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn get_page_element() -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn get_element_by_unique_id(_: i64) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn add_event_listener(_: ExternRef, _: i32, _: i32, _: ExternRef, _: ExternRef) {}
    pub unsafe fn remove_event_listener(_: ExternRef, _: i32, _: i32, _: ExternRef, _: ExternRef) {}
    pub unsafe fn create_event(_: i32, _: i32, _: i32, _: ExternRef, _: ExternRef) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn dispatch_event(_: ExternRef, _: ExternRef) -> i32 {
        0
    }
    pub unsafe fn stop_propagation(_: ExternRef) {}
    pub unsafe fn stop_immediate_propagation(_: ExternRef) {}
    pub unsafe fn invoke_ui_method(_: ExternRef, _: i32, _: i32, _: ExternRef, _: ExternRef) {}
    pub unsafe fn get_computed_style_by_key(
        _: ExternRef,
        _: i32,
        _: i32,
        _: *mut HostValueAbiOut,
        _: *mut u8,
        _: i32,
    ) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn set_timeout(_: ExternRef, _: i64) -> i64 {
        0
    }
    pub unsafe fn clear_timeout(_: i64) {}
    pub unsafe fn set_interval(_: ExternRef, _: i64) -> i64 {
        0
    }
    pub unsafe fn clear_interval(_: i64) {}
}

/// Calls `__CreateElement`.
#[inline]
pub fn create_element(tag: &str) -> ExternRef {
    let (tag_ptr, tag_len) = string_parts(tag);
    ExternRef::from_raw(unsafe { ffi::create_element(tag_ptr, tag_len) })
}

/// Calls `__CreatePage`.
#[inline]
pub fn create_page() -> ExternRef {
    ExternRef::from_raw(unsafe { ffi::create_page() })
}

macro_rules! create_parent_with_info {
    ($name:ident, $ffi_name:ident) => {
        /// Calls the matching create binding.
        #[inline]
        pub fn $name() -> ExternRef {
            ExternRef::from_raw(unsafe { ffi::$ffi_name() })
        }
    };
}

create_parent_with_info!(create_view, create_view);
create_parent_with_info!(create_scroll_view, create_scroll_view);
create_parent_with_info!(create_text, create_text);
create_parent_with_info!(create_image, create_image);

/// Calls `__CreateRawText`.
#[inline]
pub fn create_raw_text(text: &str) -> ExternRef {
    let (text_ptr, text_len) = string_parts(text);
    ExternRef::from_raw(unsafe { ffi::create_raw_text(text_ptr, text_len) })
}

/// Calls `__CreateNonElement`.
#[inline]
pub fn create_non_element() -> ExternRef {
    ExternRef::from_raw(unsafe { ffi::create_non_element() })
}

/// Calls `__CreateWrapperElement`.
#[inline]
pub fn create_wrapper_element() -> ExternRef {
    ExternRef::from_raw(unsafe { ffi::create_wrapper_element() })
}

/// Calls `binding__DropElement`.
#[inline]
pub fn drop_element(element: ExternRef) {
    unsafe { ffi::drop_element(element.raw()) }
}

macro_rules! two_ref_return_ref {
    ($name:ident, $ffi_name:ident) => {
        /// Calls the matching two-externref binding.
        #[inline]
        pub fn $name(left: ExternRef, right: ExternRef) -> ExternRef {
            unsafe { ffi::$ffi_name(left, right) }
        }
    };
}

two_ref_return_ref!(append_element, append_element);
two_ref_return_ref!(remove_element, remove_element);

/// Calls `__InsertElementBefore`.
#[inline]
pub fn insert_element_before(
    parent: ExternRef,
    child: ExternRef,
    before: HostValue<'_>,
) -> ExternRef {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(before);
    unsafe {
        ffi::insert_element_before(
            parent, child, kind, number, string_ptr, string_len, ref_value,
        )
    }
}

macro_rules! one_ref_return_ref {
    ($name:ident, $ffi_name:ident) => {
        /// Calls the matching one-externref binding.
        #[inline]
        pub fn $name(element: ExternRef) -> ExternRef {
            unsafe { ffi::$ffi_name(element) }
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
pub fn replace_element(new_element: ExternRef, old_element: ExternRef) {
    unsafe { ffi::replace_element(new_element, old_element) }
}

/// Calls `__SwapElement`.
pub fn swap_element(left: ExternRef, right: ExternRef) {
    unsafe { ffi::swap_element(left, right) }
}

/// Calls `__ElementIsEqual`.
pub fn element_is_equal(left: ExternRef, right: ExternRef) -> bool {
    unsafe { ffi::element_is_equal(left, right) != 0 }
}

/// Calls `__GetElementUniqueID`.
pub fn get_element_unique_id(element: ExternRef) -> i64 {
    unsafe { ffi::get_element_unique_id(element) }
}

/// Calls `__GetTag`.
///
/// Thin raw-ABI wrapper: the out-pointer/length describe a guest-owned buffer
/// (today always supplied by [`string_out_call`]); the host only writes within
/// `out_max_len`.
#[inline]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn get_tag(element: ExternRef, out_ptr: *mut u8, out_max_len: i32) -> i32 {
    unsafe { ffi::get_tag(element, out_ptr, out_max_len) }
}

/// Calls `__SetAttribute`.
#[inline]
pub fn set_attribute(element: ExternRef, key: HostValue<'_>, value: HostValue<'_>) {
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
pub fn add_class(element: ExternRef, class_name: &str) {
    let (ptr, len) = string_parts(class_name);
    unsafe { ffi::add_class(element, ptr, len) }
}

/// Calls `__SetClasses`.
#[inline]
pub fn set_classes(element: ExternRef, classes: &str) {
    let (ptr, len) = string_parts(classes);
    unsafe { ffi::set_classes(element, ptr, len) }
}

/// Calls `__AddInlineStyle`.
#[inline]
pub fn add_inline_style(element: ExternRef, key: HostValue<'_>, value: HostValue<'_>) {
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
pub fn set_inline_styles(element: ExternRef, value: HostValue<'_>) {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe { ffi::set_inline_styles(element, kind, number, string_ptr, string_len, ref_value) }
}

/// Calls `__GetInlineStyles`.
///
/// Thin raw-ABI wrapper: the out-pointer/length describe a guest-owned buffer;
/// the host only writes within `out_max_len`.
#[inline]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn get_inline_styles(element: ExternRef, out_ptr: *mut u8, out_max_len: i32) -> i32 {
    unsafe { ffi::get_inline_styles(element, out_ptr, out_max_len) }
}

/// Calls `__SetParsedStyles`.
pub fn set_parsed_styles(element: ExternRef, styles: &str, config: HostValue<'_>) {
    let (styles_ptr, styles_len) = string_parts(styles);
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(config);
    unsafe {
        ffi::set_parsed_styles(
            element, styles_ptr, styles_len, kind, number, string_ptr, string_len, ref_value,
        )
    }
}

/// Calls `__GetComputedStyles`.
pub fn get_computed_styles(element: ExternRef) -> HostValueOut {
    any_return(|out, string_ptr, string_len| unsafe {
        ffi::get_computed_styles(element, out, string_ptr, string_len)
    })
}

/// Calls `__AddEvent`.
#[inline]
pub fn add_event(element: ExternRef, name: &str, event_type: &str, value: HostValue<'_>) {
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
pub fn set_events(element: ExternRef, value: HostValue<'_>) {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe { ffi::set_events(element, kind, number, string_ptr, string_len, ref_value) }
}

/// Calls `__GetEvent`.
pub fn get_event(element: ExternRef, name: &str, event_type: &str) -> ExternRef {
    let (name_ptr, name_len) = string_parts(name);
    let (type_ptr, type_len) = string_parts(event_type);
    unsafe { ffi::get_event(element, name_ptr, name_len, type_ptr, type_len) }
}

/// Calls `__SetID`.
#[inline]
pub fn set_id(element: ExternRef, value: HostValue<'_>) {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe { ffi::set_id(element, kind, number, string_ptr, string_len, ref_value) }
}

/// Calls `__GetID`.
///
/// Thin raw-ABI wrapper: the out-pointer/length describe a guest-owned buffer;
/// the host only writes within `out_max_len`.
#[inline]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn get_id(element: ExternRef, out_ptr: *mut u8, out_max_len: i32) -> i32 {
    unsafe { ffi::get_id(element, out_ptr, out_max_len) }
}

/// Calls `__AddDataset`.
#[inline]
pub fn add_dataset(element: ExternRef, key: &str, value: HostValue<'_>) {
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
pub fn set_dataset(element: ExternRef, value: HostValue<'_>) {
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
pub fn get_data_by_key(element: ExternRef, key: &str) -> HostValueOut {
    let (key_ptr, key_len) = string_parts(key);
    any_return(|out, string_ptr, string_len| unsafe {
        ffi::get_data_by_key(element, key_ptr, key_len, out, string_ptr, string_len)
    })
}

/// Calls `__ReplaceElements`.
pub fn replace_elements(
    parent: ExternRef,
    new_elements: HostValue<'_>,
    old_elements: HostValue<'_>,
) {
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

/// Calls `__QuerySelector`.
pub fn query_selector(element: ExternRef, selector: &str, options: HostValue<'_>) -> ExternRef {
    let (selector_ptr, selector_len) = string_parts(selector);
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(options);
    unsafe {
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
    }
}

/// Calls `__QuerySelectorAll`.
pub fn query_selector_all(element: ExternRef, selector: &str, options: HostValue<'_>) -> ExternRef {
    let (selector_ptr, selector_len) = string_parts(selector);
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(options);
    unsafe {
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
    }
}

/// Calls `__AddConfig`.
pub fn add_config(element: ExternRef, key: &str, value: HostValue<'_>) {
    let (key_ptr, key_len) = string_parts(key);
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe {
        ffi::add_config(
            element, key_ptr, key_len, kind, number, string_ptr, string_len, ref_value,
        )
    }
}

/// Calls `__SetConfig`.
pub fn set_config(element: ExternRef, value: HostValue<'_>) {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe { ffi::set_config(element, kind, number, string_ptr, string_len, ref_value) }
}

/// Calls `__GetInlineStyle`.
pub fn get_inline_style(element: ExternRef, index: i32) -> HostValueOut {
    any_return(|out, string_ptr, string_len| unsafe {
        ffi::get_inline_style(element, index, out, string_ptr, string_len)
    })
}

/// Calls `__GetAttributeByName`.
pub fn get_attribute_by_name(element: ExternRef, key: &str) -> HostValueOut {
    let (key_ptr, key_len) = string_parts(key);
    any_return(|out, string_ptr, string_len| unsafe {
        ffi::get_attribute_by_name(element, key_ptr, key_len, out, string_ptr, string_len)
    })
}

/// Borrowing variant of [`get_attribute_by_name`] that lends the value to `f`
/// without allocating an owned string.
#[inline]
pub(crate) fn get_attribute_by_name_borrow<R>(
    element: ExternRef,
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

/// Calls `__GetPageElement`.
pub fn get_page_element() -> ExternRef {
    unsafe { ffi::get_page_element() }
}

/// Calls `__GetElementByUniqueID`.
pub fn get_element_by_unique_id(unique_id: i64) -> ExternRef {
    unsafe { ffi::get_element_by_unique_id(unique_id) }
}

/// Calls `__AddEventListener`.
pub fn add_event_listener(
    element: ExternRef,
    event_type: &str,
    listener: ExternRef,
    options: ExternRef,
) {
    let (event_type_ptr, event_type_len) = string_parts(event_type);
    unsafe { ffi::add_event_listener(element, event_type_ptr, event_type_len, listener, options) }
}

/// Calls `__RemoveEventListener`.
pub fn remove_event_listener(
    element: ExternRef,
    event_type: &str,
    listener: ExternRef,
    options: ExternRef,
) {
    let (event_type_ptr, event_type_len) = string_parts(event_type);
    unsafe {
        ffi::remove_event_listener(element, event_type_ptr, event_type_len, listener, options)
    }
}

/// Calls `__CreateEvent`.
pub fn create_event(
    event_id: i32,
    event_type: &str,
    target: ExternRef,
    options: ExternRef,
) -> ExternRef {
    let (event_type_ptr, event_type_len) = string_parts(event_type);
    unsafe { ffi::create_event(event_id, event_type_ptr, event_type_len, target, options) }
}

/// Calls `__DispatchEvent`.
pub fn dispatch_event(element: ExternRef, event: ExternRef) -> bool {
    unsafe { ffi::dispatch_event(element, event) != 0 }
}

/// Calls `__StopPropagation`.
pub fn stop_propagation(event: ExternRef) {
    unsafe { ffi::stop_propagation(event) }
}

/// Calls `__StopImmediatePropagation`.
pub fn stop_immediate_propagation(event: ExternRef) {
    unsafe { ffi::stop_immediate_propagation(event) }
}

/// Calls `__InvokeUIMethod`.
pub fn invoke_ui_method(element: ExternRef, method: &str, params: ExternRef, callback: ExternRef) {
    let (method_ptr, method_len) = string_parts(method);
    unsafe { ffi::invoke_ui_method(element, method_ptr, method_len, params, callback) }
}

/// Calls `__GetComputedStyleByKey`.
pub fn get_computed_style_by_key(element: ExternRef, key: &str) -> HostValueOut {
    let (key_ptr, key_len) = string_parts(key);
    any_return(|out, string_ptr, string_len| unsafe {
        ffi::get_computed_style_by_key(element, key_ptr, key_len, out, string_ptr, string_len)
    })
}

/// Calls `setTimeout`.
pub fn set_timeout(callback: ExternRef, delay_ms: i64) -> i64 {
    unsafe { ffi::set_timeout(callback, delay_ms) }
}

/// Calls `clearTimeout`.
pub fn clear_timeout(timer_id: i64) {
    unsafe { ffi::clear_timeout(timer_id) }
}

/// Calls `setInterval`.
pub fn set_interval(callback: ExternRef, delay_ms: i64) -> i64 {
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
            ExternRef::null()
        };
        assert_eq!(any_return(null), HostValueOut::Null);

        let number = |out: *mut HostValueAbiOut, _ptr: *mut u8, _max: i32| {
            unsafe {
                (*out).kind = HostValueKind::Number as i32;
                (*out).number_value = 42.5;
            }
            ExternRef::null()
        };
        assert_eq!(any_return(number), HostValueOut::Number(42.5));
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
            ExternRef::null()
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
            ExternRef::null()
        };
        assert!(any_borrow_call(str_call, |value| match value {
            AnyBorrow::Str(bytes) => bytes == b"http://www.w3.org/2000/svg",
            _ => false,
        }));

        let null_call = |out: *mut HostValueAbiOut, _ptr: *mut u8, _max: i32| {
            unsafe { (*out).kind = HostValueKind::Null as i32 };
            ExternRef::null()
        };
        assert!(any_borrow_call(null_call, |value| matches!(
            value,
            AnyBorrow::Null
        )));
    }
}
