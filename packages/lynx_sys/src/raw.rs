//! Raw wrappers for the WAMR `env` host-function ABI.

use std::cell::RefCell;

/// Sentinel host handle meaning "no node".
///
/// The host allocates every element/event in typed arenas and returns its
/// non-negative arena id; a negative id means "absent".
pub const NULL_NODE: i32 = -1;

/// Event option bit: capture.
pub const EVENT_FLAG_CAPTURE: i32 = 1 << 0;
/// Event option bit: bubbles.
pub const EVENT_FLAG_BUBBLES: i32 = 1 << 1;
/// Event option bit: cancelable.
pub const EVENT_FLAG_CANCELABLE: i32 = 1 << 2;
/// Event option bit: composed.
pub const EVENT_FLAG_COMPOSED: i32 = 1 << 3;

/// Event listener option bit: capture.
pub const LISTENER_FLAG_CAPTURE: i32 = 1 << 0;
/// Event listener option bit: once.
pub const LISTENER_FLAG_ONCE: i32 = 1 << 1;
/// Event listener option bit: passive.
pub const LISTENER_FLAG_PASSIVE: i32 = 1 << 2;

/// Converts a raw host arena id into `Option`.
#[inline(always)]
fn node(raw: i32) -> Option<i32> {
    (raw >= 0).then_some(raw)
}

/// Initial capacity for the reusable host-string scratch buffer.
const SCRATCH_INITIAL_CAPACITY: usize = 1024;
const ARRAY_INITIAL_CAPACITY: usize = 32;

thread_local! {
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Result of [`string_out_call`].
pub(crate) enum StringOut {
    /// The host reported the value is absent (negative sentinel).
    Absent,
    /// Exact-capacity bytes copied out of the scratch buffer.
    Bytes(Vec<u8>),
    /// The host re-reported a required length larger than the grown capacity.
    Overflow {
        /// Required byte length reported by the host on retry.
        required: usize,
        /// Capacity offered on retry.
        capacity: usize,
    },
}

fn ensure_capacity(buf: &mut Vec<u8>, min: usize) {
    if buf.capacity() < min {
        buf.reserve(min - buf.len());
    }
    // SAFETY: `u8` has no invalid bit patterns, and callers only read the
    // bytes the host reports as initialized.
    unsafe {
        buf.set_len(buf.capacity());
    }
}

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

/// Like [`string_out_call`] but lends the written bytes to `f`.
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

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct StringSlice {
    offset: i32,
    len: i32,
}

fn i32_array_out_call(call: impl Fn(*mut i32, i32) -> i32) -> Vec<i32> {
    let mut out = vec![0; ARRAY_INITIAL_CAPACITY];
    let required = call(out.as_mut_ptr(), out.len() as i32);
    if required <= 0 {
        return Vec::new();
    }
    let required = required as usize;
    if required > out.len() {
        out.resize(required, 0);
        let retry = call(out.as_mut_ptr(), out.len() as i32);
        if retry <= 0 {
            return Vec::new();
        }
        let retry = retry as usize;
        if retry > out.len() {
            return Vec::new();
        }
        out.truncate(retry);
        return out;
    }
    out.truncate(required);
    out
}

fn string_array_out_call(
    call: impl Fn(*mut StringSlice, i32, *mut u8, i32, *mut i32) -> i32,
) -> Vec<String> {
    let mut items = vec![StringSlice::default(); ARRAY_INITIAL_CAPACITY];
    let mut bytes = vec![0; SCRATCH_INITIAL_CAPACITY];
    let mut required_bytes = 0;
    let mut required_items = call(
        items.as_mut_ptr(),
        items.len() as i32,
        bytes.as_mut_ptr(),
        bytes.len() as i32,
        &mut required_bytes,
    );
    if required_items <= 0 {
        return Vec::new();
    }
    if required_items as usize > items.len() || required_bytes as usize > bytes.len() {
        items.resize(required_items.max(0) as usize, StringSlice::default());
        bytes.resize(required_bytes.max(0) as usize, 0);
        required_items = call(
            items.as_mut_ptr(),
            items.len() as i32,
            bytes.as_mut_ptr(),
            bytes.len() as i32,
            &mut required_bytes,
        );
    }
    if required_items <= 0 || required_items as usize > items.len() {
        return Vec::new();
    }
    items
        .into_iter()
        .take(required_items as usize)
        .filter_map(|slice| {
            if slice.offset < 0 || slice.len < 0 {
                return None;
            }
            let start = slice.offset as usize;
            let end = start.checked_add(slice.len as usize)?;
            bytes
                .get(start..end)
                .map(|value| String::from_utf8_lossy(value).into_owned())
        })
        .collect()
}

mod ffi {
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
        #[link_name = "binding__DropEvent"]
        #[cfg_attr(all(test, not(target_arch = "wasm32")), allow(dead_code))]
        pub fn drop_event(event_id: i32);
        #[link_name = "__AppendElement"]
        pub fn append_element(parent: i32, child: i32) -> i32;
        #[link_name = "__RemoveElement"]
        pub fn remove_element(parent: i32, child: i32) -> i32;
        #[link_name = "__InsertElementBefore"]
        pub fn insert_element_before(parent: i32, child: i32, before: i32) -> i32;
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
        pub fn get_children(element: i32, out_ptr: *mut i32, out_cap: i32) -> i32;
        #[link_name = "__ElementIsEqual"]
        pub fn element_is_equal(left: i32, right: i32) -> i32;
        #[link_name = "__GetElementUniqueID"]
        pub fn get_element_unique_id(element: i32) -> i64;
        #[link_name = "__GetTag"]
        pub fn get_tag(element: i32, out_ptr: *mut u8, out_max_len: i32) -> i32;
        #[link_name = "__SetStringAttribute"]
        pub fn set_string_attribute(
            element: i32,
            key_ptr: i32,
            key_len: i32,
            value_ptr: i32,
            value_len: i32,
        );
        #[link_name = "__SetInlineStyleText"]
        pub fn set_inline_style_text(element: i32, value_ptr: i32, value_len: i32);
        #[link_name = "__RemoveAttribute"]
        pub fn remove_attribute(element: i32, key_ptr: i32, key_len: i32);
        #[link_name = "__AdoptStyleSheetTokens"]
        pub fn adopt_style_sheet_tokens(bytes_ptr: i32, bytes_len: i32);
        #[link_name = "__ReplaceStyleSheetsTokens"]
        pub fn replace_style_sheets_tokens(bytes_ptr: i32, bytes_len: i32);
        #[link_name = "__AddClass"]
        pub fn add_class(element: i32, class_ptr: i32, class_len: i32);
        #[link_name = "__SetClasses"]
        pub fn set_classes(element: i32, classes_ptr: i32, classes_len: i32);
        #[link_name = "__GetClasses"]
        pub fn get_classes(
            element: i32,
            items_ptr: *mut super::StringSlice,
            item_cap: i32,
            bytes_ptr: *mut u8,
            byte_cap: i32,
            required_bytes: *mut i32,
        ) -> i32;
        #[link_name = "__SetID"]
        pub fn set_id(element: i32, value_ptr: i32, value_len: i32);
        #[link_name = "__GetID"]
        pub fn get_id(element: i32, out_ptr: *mut u8, out_max_len: i32) -> i32;
        #[link_name = "__FlushElementTree"]
        pub fn flush_element_tree(root: i32);
        #[link_name = "__ReplaceElements"]
        pub fn replace_elements(
            parent: i32,
            inserted_ptr: *const i32,
            inserted_len: i32,
            removed_ptr: *const i32,
            removed_len: i32,
            ref_id: i32,
        );
        #[link_name = "__QuerySelector"]
        pub fn query_selector(
            element: i32,
            selector_ptr: i32,
            selector_len: i32,
            only_current_component: i32,
        ) -> i32;
        #[link_name = "__QuerySelectorAll"]
        pub fn query_selector_all(
            element: i32,
            selector_ptr: i32,
            selector_len: i32,
            out_ptr: *mut i32,
            out_cap: i32,
            only_current_component: i32,
        ) -> i32;
        #[link_name = "__GetStringAttributeByName"]
        pub fn get_string_attribute_by_name(
            element: i32,
            key_ptr: i32,
            key_len: i32,
            out_ptr: *mut u8,
            out_max_len: i32,
        ) -> i32;
        #[link_name = "__GetAttributeNames"]
        pub fn get_attribute_names(
            element: i32,
            items_ptr: *mut super::StringSlice,
            item_cap: i32,
            bytes_ptr: *mut u8,
            byte_cap: i32,
            required_bytes: *mut i32,
        ) -> i32;
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
        pub fn create_event(event_type: i32, name_ptr: i32, name_len: i32, flags: i32) -> i32;
        #[link_name = "__DispatchEvent"]
        pub fn dispatch_event(element: i32, event: i32) -> i32;
        #[link_name = "__StopPropagation"]
        pub fn stop_propagation(event: i32);
        #[link_name = "__StopImmediatePropagation"]
        pub fn stop_immediate_propagation(event: i32);
        #[link_name = "__GetEventType"]
        pub fn get_event_type(event: i32, out_ptr: *mut u8, out_max_len: i32) -> i32;
        #[link_name = "__GetEventCurrentTargetUniqueID"]
        pub fn get_event_current_target_unique_id(event: i32) -> i64;
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

/// Calls `__CreateElement`.
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

/// Calls `__CreateRawText`.
#[inline]
pub fn create_raw_text(text: &str) -> i32 {
    let (text_ptr, text_len) = string_parts(text);
    unsafe { ffi::create_raw_text(text_ptr, text_len) }
}

#[inline]
pub fn create_non_element() -> i32 {
    unsafe { ffi::create_non_element() }
}

#[inline]
pub fn create_wrapper_element() -> i32 {
    unsafe { ffi::create_wrapper_element() }
}

#[inline]
pub fn drop_element(element: i32) {
    unsafe { ffi::drop_element(element) }
}

#[inline]
#[cfg(not(all(test, not(target_arch = "wasm32"))))]
pub fn drop_event(event: i32) {
    unsafe { ffi::drop_event(event) }
}

#[inline]
#[cfg(all(test, not(target_arch = "wasm32")))]
pub fn drop_event(_event: i32) {}

macro_rules! two_ref_return_ref {
    ($name:ident, $ffi_name:ident) => {
        #[inline]
        pub fn $name(left: i32, right: i32) -> i32 {
            unsafe { ffi::$ffi_name(left, right) }
        }
    };
}

two_ref_return_ref!(append_element, append_element);
two_ref_return_ref!(remove_element, remove_element);

#[inline]
pub fn insert_element_before(parent: i32, child: i32, before: Option<i32>) -> i32 {
    unsafe { ffi::insert_element_before(parent, child, before.unwrap_or(NULL_NODE)) }
}

macro_rules! one_ref_return_ref {
    ($name:ident, $ffi_name:ident) => {
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

#[inline]
pub fn get_children(element: i32) -> Vec<i32> {
    i32_array_out_call(|ptr, cap| unsafe { ffi::get_children(element, ptr, cap) })
}

pub fn replace_element(new_element: i32, old_element: i32) {
    unsafe { ffi::replace_element(new_element, old_element) }
}

pub fn swap_element(left: i32, right: i32) {
    unsafe { ffi::swap_element(left, right) }
}

pub fn element_is_equal(left: i32, right: i32) -> bool {
    unsafe { ffi::element_is_equal(left, right) != 0 }
}

pub fn get_element_unique_id(element: i32) -> i64 {
    unsafe { ffi::get_element_unique_id(element) }
}

#[inline]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn get_tag(element: i32, out_ptr: *mut u8, out_max_len: i32) -> i32 {
    unsafe { ffi::get_tag(element, out_ptr, out_max_len) }
}

#[inline]
pub fn set_string_attribute(element: i32, key: &str, value: &str) {
    let (key_ptr, key_len) = string_parts(key);
    let (value_ptr, value_len) = string_parts(value);
    unsafe { ffi::set_string_attribute(element, key_ptr, key_len, value_ptr, value_len) }
}

#[inline]
pub fn set_inline_style_text(element: i32, value: &str) {
    let (value_ptr, value_len) = string_parts(value);
    unsafe { ffi::set_inline_style_text(element, value_ptr, value_len) }
}

#[inline]
pub fn remove_attribute(element: i32, key: &str) {
    let (key_ptr, key_len) = string_parts(key);
    unsafe { ffi::remove_attribute(element, key_ptr, key_len) }
}

#[inline]
pub fn adopt_style_sheet_tokens(tokens: crate::css::CSSTokenStream) {
    unsafe { ffi::adopt_style_sheet_tokens(tokens.as_ptr() as i32, tokens.len() as i32) }
}

#[inline]
pub fn replace_style_sheets_tokens(tokens: crate::css::CSSTokenStream) {
    unsafe { ffi::replace_style_sheets_tokens(tokens.as_ptr() as i32, tokens.len() as i32) }
}

#[inline]
pub fn add_class(element: i32, class_name: &str) {
    let (ptr, len) = string_parts(class_name);
    unsafe { ffi::add_class(element, ptr, len) }
}

#[inline]
pub fn set_classes(element: i32, classes: &str) {
    let (ptr, len) = string_parts(classes);
    unsafe { ffi::set_classes(element, ptr, len) }
}

pub fn get_classes(element: i32) -> Vec<String> {
    string_array_out_call(|items, item_cap, bytes, byte_cap, required_bytes| unsafe {
        ffi::get_classes(element, items, item_cap, bytes, byte_cap, required_bytes)
    })
}

pub fn set_id(element: i32, value: Option<&str>) {
    match value {
        Some(value) => {
            let (ptr, len) = string_parts(value);
            unsafe { ffi::set_id(element, ptr, len) }
        }
        None => unsafe { ffi::set_id(element, NULL_NODE, NULL_NODE) },
    }
}

#[inline]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn get_id(element: i32, out_ptr: *mut u8, out_max_len: i32) -> i32 {
    unsafe { ffi::get_id(element, out_ptr, out_max_len) }
}

pub fn flush_element_tree(root: Option<i32>) {
    unsafe { ffi::flush_element_tree(root.unwrap_or(NULL_NODE)) }
}

pub fn replace_elements(parent: i32, inserted: &[i32], removed: &[i32], ref_id: Option<i32>) {
    unsafe {
        ffi::replace_elements(
            parent,
            inserted.as_ptr(),
            inserted.len() as i32,
            removed.as_ptr(),
            removed.len() as i32,
            ref_id.unwrap_or(NULL_NODE),
        )
    }
}

pub fn query_selector(element: i32, selector: &str, only_current_component: bool) -> Option<i32> {
    let (selector_ptr, selector_len) = string_parts(selector);
    node(unsafe {
        ffi::query_selector(
            element,
            selector_ptr,
            selector_len,
            i32::from(only_current_component),
        )
    })
}

pub fn query_selector_all(element: i32, selector: &str, only_current_component: bool) -> Vec<i32> {
    let (selector_ptr, selector_len) = string_parts(selector);
    i32_array_out_call(|ptr, cap| unsafe {
        ffi::query_selector_all(
            element,
            selector_ptr,
            selector_len,
            ptr,
            cap,
            i32::from(only_current_component),
        )
    })
}

#[inline]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn get_string_attribute_by_name(
    element: i32,
    key: &str,
    out_ptr: *mut u8,
    out_max_len: i32,
) -> i32 {
    let (key_ptr, key_len) = string_parts(key);
    unsafe { ffi::get_string_attribute_by_name(element, key_ptr, key_len, out_ptr, out_max_len) }
}

pub fn get_attribute_names(element: i32) -> Vec<String> {
    string_array_out_call(|items, item_cap, bytes, byte_cap, required_bytes| unsafe {
        ffi::get_attribute_names(element, items, item_cap, bytes, byte_cap, required_bytes)
    })
}

pub fn get_page_element() -> Option<i32> {
    node(unsafe { ffi::get_page_element() })
}

pub fn get_element_by_unique_id(unique_id: i64) -> Option<i32> {
    node(unsafe { ffi::get_element_by_unique_id(unique_id) })
}

pub fn add_event_listener(element: i32, event_type: &str, listener: i32, options: i32) {
    let (event_type_ptr, event_type_len) = string_parts(event_type);
    unsafe { ffi::add_event_listener(element, event_type_ptr, event_type_len, listener, options) }
}

pub fn remove_event_listener(element: i32, event_type: &str, listener: i32, options: i32) {
    let (event_type_ptr, event_type_len) = string_parts(event_type);
    unsafe {
        ffi::remove_event_listener(element, event_type_ptr, event_type_len, listener, options)
    }
}

pub fn create_event(event_type: i32, name: &str, flags: i32) -> Option<i32> {
    let (name_ptr, name_len) = string_parts(name);
    node(unsafe { ffi::create_event(event_type, name_ptr, name_len, flags) })
}

pub fn dispatch_event(element: i32, event: i32) -> bool {
    unsafe { ffi::dispatch_event(element, event) != 0 }
}

pub fn stop_propagation(event: i32) {
    unsafe { ffi::stop_propagation(event) }
}

pub fn stop_immediate_propagation(event: i32) {
    unsafe { ffi::stop_immediate_propagation(event) }
}

#[inline]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn get_event_type(event: i32, out_ptr: *mut u8, out_max_len: i32) -> i32 {
    unsafe { ffi::get_event_type(event, out_ptr, out_max_len) }
}

#[inline]
pub fn get_event_current_target_unique_id(event: i32) -> Option<i64> {
    let unique_id = unsafe { ffi::get_event_current_target_unique_id(event) };
    (unique_id >= 0).then_some(unique_id)
}

pub fn set_timeout(callback: i32, delay_ms: i64) -> i64 {
    unsafe { ffi::set_timeout(callback, delay_ms) }
}

pub fn clear_timeout(timer_id: i64) {
    unsafe { ffi::clear_timeout(timer_id) }
}

pub fn set_interval(callback: i32, delay_ms: i64) -> i64 {
    unsafe { ffi::set_interval(callback, delay_ms) }
}

pub fn clear_interval(timer_id: i64) {
    unsafe { ffi::clear_interval(timer_id) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer(payload: &'static [u8]) -> impl Fn(*mut u8, i32) -> i32 {
        move |ptr, max| {
            let n = payload.len();
            if (max as usize) >= n {
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
    fn string_borrow_lends_without_owning() {
        let owned = string_borrow_call(writer(b"svg"), |bytes| bytes.map(<[u8]>::to_vec));
        assert_eq!(owned, Some(b"svg".to_vec()));
    }

    #[test]
    fn string_borrow_absent_on_negative() {
        assert!(string_borrow_call(|_ptr, _max| -1, |bytes| bytes.is_none()));
    }

    #[test]
    fn i32_array_out_grows_and_retries() {
        let values = [1, 2, 3, 4, 5];
        let out = i32_array_out_call(|ptr, cap| {
            if cap as usize >= values.len() {
                unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), ptr, values.len()) };
            }
            values.len() as i32
        });
        assert_eq!(out, values);
    }

    #[test]
    fn string_array_out_decodes_slices() {
        let out = string_array_out_call(|items, item_cap, bytes, byte_cap, required_bytes| {
            unsafe {
                *required_bytes = 6;
                if item_cap >= 2 && byte_cap >= 6 {
                    std::ptr::copy_nonoverlapping(b"foobar".as_ptr(), bytes, 6);
                    *items.add(0) = StringSlice { offset: 0, len: 3 };
                    *items.add(1) = StringSlice { offset: 3, len: 3 };
                }
            }
            2
        });
        assert_eq!(out, vec!["foo", "bar"]);
    }
}
