//! Raw wrappers for the WAMR `env` host-function ABI.

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
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns a null host reference.
    pub const fn null() -> Self {
        Self(0)
    }

    /// Returns the raw ABI carrier.
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Returns true when this reference is null.
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

/// Guest callback shape used by host timers.
///
/// The argument is the native timer id returned by `setTimeout` /
/// `setInterval`. The host stores only the function-table index and calls it
/// back through WAMR `call_indirect`.
pub type TimerCallback = extern "C" fn(i32);

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

const ANY_STRING_CAPACITY: usize = 16 * 1024;

fn string_parts(value: &str) -> (i32, i32) {
    (value.as_ptr() as i32, value.len() as i32)
}

fn decode_any_return(raw_ref: ExternRef, out: HostValueAbiOut, bytes: Vec<u8>) -> HostValueOut {
    match out.kind {
        value if value == HostValueKind::Undefined as i32 => HostValueOut::Undefined,
        value if value == HostValueKind::Null as i32 => HostValueOut::Null,
        value if value == HostValueKind::Bool as i32 => HostValueOut::Bool(out.bool_value != 0),
        value if value == HostValueKind::Number as i32 => HostValueOut::Number(out.number_value),
        value if value == HostValueKind::String as i32 => {
            let written_len = out.string_written_length.max(0) as usize;
            let mut bytes = bytes;
            bytes.truncate(written_len.min(bytes.len()));
            HostValueOut::String {
                bytes,
                required_len: out.string_required_length.max(0) as usize,
            }
        }
        value if value == HostValueKind::ExternRef as i32 => HostValueOut::ExternRef(raw_ref),
        _ => HostValueOut::Undefined,
    }
}

fn any_return(f: impl FnOnce(*mut HostValueAbiOut, *mut u8, i32) -> ExternRef) -> HostValueOut {
    let mut out = HostValueAbiOut::default();
    let mut string_buffer = vec![0; ANY_STRING_CAPACITY];
    let raw_ref = f(
        &mut out,
        string_buffer.as_mut_ptr(),
        string_buffer.len() as i32,
    );
    decode_any_return(raw_ref, out, string_buffer)
}

macro_rules! any_args {
    ($value:expr) => {
        $value.into_raw_parts()
    };
}

#[cfg(target_arch = "wasm32")]
mod ffi {
    use super::{ExternRef, HostValueAbiOut, TimerCallback};

    #[link(wasm_import_module = "env")]
    extern "C" {
        #[link_name = "__CreateElement"]
        pub fn create_element(tag_ptr: i32, tag_len: i32) -> ExternRef;
        #[link_name = "__CreatePage"]
        pub fn create_page() -> ExternRef;
        #[link_name = "__CreateView"]
        pub fn create_view() -> ExternRef;
        #[link_name = "__CreateScrollView"]
        pub fn create_scroll_view() -> ExternRef;
        #[link_name = "__CreateText"]
        pub fn create_text() -> ExternRef;
        #[link_name = "__CreateImage"]
        pub fn create_image() -> ExternRef;
        #[link_name = "__CreateRawText"]
        pub fn create_raw_text(text_ptr: i32, text_len: i32) -> ExternRef;
        #[link_name = "__CreateNonElement"]
        pub fn create_non_element() -> ExternRef;
        #[link_name = "__CreateWrapperElement"]
        pub fn create_wrapper_element() -> ExternRef;
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
        pub fn set_timeout(callback: TimerCallback, delay_ms: i64) -> i64;
        #[link_name = "clearTimeout"]
        pub fn clear_timeout(timer_id: i64);
        #[link_name = "setInterval"]
        pub fn set_interval(callback: TimerCallback, delay_ms: i64) -> i64;
        #[link_name = "clearInterval"]
        pub fn clear_interval(timer_id: i64);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
mod ffi {
    use super::{ExternRef, HostValueAbiOut, TimerCallback};

    pub unsafe fn create_element(_: i32, _: i32) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn create_page() -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn create_view() -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn create_scroll_view() -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn create_text() -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn create_image() -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn create_raw_text(_: i32, _: i32) -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn create_non_element() -> ExternRef {
        ExternRef::null()
    }
    pub unsafe fn create_wrapper_element() -> ExternRef {
        ExternRef::null()
    }
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
    pub unsafe fn set_timeout(_: TimerCallback, _: i64) -> i64 {
        0
    }
    pub unsafe fn clear_timeout(_: i64) {}
    pub unsafe fn set_interval(_: TimerCallback, _: i64) -> i64 {
        0
    }
    pub unsafe fn clear_interval(_: i64) {}
}

/// Calls `__CreateElement`.
pub fn create_element(tag: &str) -> ExternRef {
    let (tag_ptr, tag_len) = string_parts(tag);
    unsafe { ffi::create_element(tag_ptr, tag_len) }
}

/// Calls `__CreatePage`.
pub fn create_page() -> ExternRef {
    unsafe { ffi::create_page() }
}

macro_rules! create_parent_with_info {
    ($name:ident, $ffi_name:ident) => {
        /// Calls the matching create binding.
        pub fn $name() -> ExternRef {
            unsafe { ffi::$ffi_name() }
        }
    };
}

create_parent_with_info!(create_view, create_view);
create_parent_with_info!(create_scroll_view, create_scroll_view);
create_parent_with_info!(create_text, create_text);
create_parent_with_info!(create_image, create_image);

/// Calls `__CreateRawText`.
pub fn create_raw_text(text: &str) -> ExternRef {
    let (text_ptr, text_len) = string_parts(text);
    unsafe { ffi::create_raw_text(text_ptr, text_len) }
}

/// Calls `__CreateNonElement`.
pub fn create_non_element() -> ExternRef {
    unsafe { ffi::create_non_element() }
}

/// Calls `__CreateWrapperElement`.
pub fn create_wrapper_element() -> ExternRef {
    unsafe { ffi::create_wrapper_element() }
}

macro_rules! two_ref_return_ref {
    ($name:ident, $ffi_name:ident) => {
        /// Calls the matching two-externref binding.
        pub fn $name(left: ExternRef, right: ExternRef) -> ExternRef {
            unsafe { ffi::$ffi_name(left, right) }
        }
    };
}

two_ref_return_ref!(append_element, append_element);
two_ref_return_ref!(remove_element, remove_element);

/// Calls `__InsertElementBefore`.
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
pub fn get_tag(element: ExternRef, out_ptr: *mut u8, out_max_len: i32) -> i32 {
    unsafe { ffi::get_tag(element, out_ptr, out_max_len) }
}

/// Calls `__SetAttribute`.
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
pub fn add_class(element: ExternRef, class_name: &str) {
    let (ptr, len) = string_parts(class_name);
    unsafe { ffi::add_class(element, ptr, len) }
}

/// Calls `__SetClasses`.
pub fn set_classes(element: ExternRef, classes: &str) {
    let (ptr, len) = string_parts(classes);
    unsafe { ffi::set_classes(element, ptr, len) }
}

/// Calls `__AddInlineStyle`.
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
pub fn set_inline_styles(element: ExternRef, value: HostValue<'_>) {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe { ffi::set_inline_styles(element, kind, number, string_ptr, string_len, ref_value) }
}

/// Calls `__GetInlineStyles`.
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
pub fn set_id(element: ExternRef, value: HostValue<'_>) {
    let (kind, number, string_ptr, string_len, ref_value) = any_args!(value);
    unsafe { ffi::set_id(element, kind, number, string_ptr, string_len, ref_value) }
}

/// Calls `__GetID`.
pub fn get_id(element: ExternRef, out_ptr: *mut u8, out_max_len: i32) -> i32 {
    unsafe { ffi::get_id(element, out_ptr, out_max_len) }
}

/// Calls `__AddDataset`.
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
pub fn set_timeout(callback: TimerCallback, delay_ms: i64) -> i64 {
    unsafe { ffi::set_timeout(callback, delay_ms) }
}

/// Calls `clearTimeout`.
pub fn clear_timeout(timer_id: i64) {
    unsafe { ffi::clear_timeout(timer_id) }
}

/// Calls `setInterval`.
pub fn set_interval(callback: TimerCallback, delay_ms: i64) -> i64 {
    unsafe { ffi::set_interval(callback, delay_ms) }
}

/// Calls `clearInterval`.
pub fn clear_interval(timer_id: i64) {
    unsafe { ffi::clear_interval(timer_id) }
}
