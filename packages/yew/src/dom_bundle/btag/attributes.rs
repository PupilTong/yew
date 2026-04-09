use std::collections::HashMap;
use std::ops::Deref;

use indexmap::IndexMap;
use yew::AttrValue;

use super::Apply;
use crate::dom_bundle::BSubtree;
use crate::virtual_dom::vtag::{InputFields, InputMarker, TextareaFields, TextareaMarker, Value};
use crate::virtual_dom::{AttributeOrProperty, Attributes};

// In the browser fork Value<T> stored its "current DOM value" by calling the
// element's `.value()` getter. On Paws there is no property accessor — DOM
// properties and attributes collapse into a single `set_attribute` call
// (documented as a placeholder in the fork's Phase 2 plan) — so the `Value`
// apply paths simply forward the new string through the attribute path.

impl<T> Apply for Value<T> {
    type Bundle = Self;
    type Element = i32;

    fn apply(self, _root: &BSubtree, el: Self::Element) -> Self {
        if let Some(v) = self.deref() {
            set_value_placeholder(el, v);
        }
        self
    }

    fn apply_diff(self, _root: &BSubtree, el: Self::Element, bundle: &mut Self) {
        match (self.deref(), (*bundle).deref()) {
            (Some(new), Some(old)) => {
                if new != old {
                    set_value_placeholder(el, new);
                }
            }
            (Some(new), None) => set_value_placeholder(el, new),
            (None, Some(_)) => set_value_placeholder(el, ""),
            (None, None) => (),
        }
    }
}

/// Placeholder for input/textarea `.value = …`. Paws has no `set_property`
/// host function yet, so we route through `set_attribute("value", …)`. This
/// loses the DOM-property/attribute distinction but keeps the API shape.
fn set_value_placeholder(el: i32, value: &str) {
    if let Err(err) = rust_wasm_binding::set_attribute(el, "value", value) {
        tracing::warn!(?err, el, "failed to set `value` attribute");
    }
}

fn set_checked_placeholder(el: i32, checked: bool) {
    let value = if checked { "true" } else { "false" };
    if let Err(err) = rust_wasm_binding::set_attribute(el, "checked", value) {
        tracing::warn!(?err, el, "failed to set `checked` attribute");
    }
}

impl Apply for InputFields {
    type Bundle = Self;
    type Element = i32;

    fn apply(mut self, root: &BSubtree, el: Self::Element) -> Self {
        // IMPORTANT! This parameter has to be set every time it's explicitly given
        // to prevent strange behaviour in the browser when the DOM changes
        if let Some(checked) = self.checked {
            set_checked_placeholder(el, checked);
        }

        self.value = self.value.apply(root, el);
        self
    }

    fn apply_diff(self, root: &BSubtree, el: Self::Element, bundle: &mut Self) {
        if let Some(checked) = self.checked {
            set_checked_placeholder(el, checked);
        }

        self.value.apply_diff(root, el, &mut bundle.value);
    }
}

impl Apply for TextareaFields {
    type Bundle = Value<TextareaMarker>;
    type Element = i32;

    fn apply(self, root: &BSubtree, el: Self::Element) -> Self::Bundle {
        if let Some(def) = self.defaultvalue {
            if let Err(err) = rust_wasm_binding::set_attribute(el, "defaultValue", def.as_str()) {
                tracing::warn!(?err, el, "failed to set `defaultValue` attribute");
            }
        }
        self.value.apply(root, el)
    }

    fn apply_diff(self, root: &BSubtree, el: Self::Element, bundle: &mut Self::Bundle) {
        self.value.apply_diff(root, el, bundle)
    }
}

// Silence unused-import warnings from the marker type aliases — the markers
// are load-bearing for type inference on `Value<T>` but aren't referenced by
// name outside the macro-expanded call sites.
#[allow(dead_code)]
const _: Option<InputMarker> = None;

impl Attributes {
    #[cold]
    fn apply_diff_index_maps(
        el: i32,
        new: &IndexMap<AttrValue, AttributeOrProperty>,
        old: &IndexMap<AttrValue, AttributeOrProperty>,
    ) {
        for (key, value) in new.iter() {
            match old.get(key) {
                Some(old_value) => {
                    if value != old_value {
                        Self::set(el, key, value);
                    }
                }
                None => Self::set(el, key, value),
            }
        }

        for (key, value) in old.iter() {
            if !new.contains_key(key) {
                Self::remove(el, key, value);
            }
        }
    }

    /// Convert [Attributes] pair to [HashMap]s and patch changes to `el`.
    /// Works with any [Attributes] variants.
    #[cold]
    fn apply_diff_as_maps<'a>(el: i32, new: &'a Self, old: &'a Self) {
        fn collect(src: &Attributes) -> HashMap<&str, &AttributeOrProperty> {
            use Attributes::*;

            match src {
                Static(arr) => (*arr).iter().map(|(k, v)| (*k, v)).collect(),
                Dynamic { keys, values } => keys
                    .iter()
                    .zip(values.iter())
                    .filter_map(|(k, v)| v.as_ref().map(|v| (*k, v)))
                    .collect(),
                IndexMap(m) => m.iter().map(|(k, v)| (k.as_ref(), v)).collect(),
            }
        }

        let new = collect(new);
        let old = collect(old);

        // Update existing or set new
        for (k, new) in new.iter() {
            if match old.get(k) {
                Some(old) => old != new,
                None => true,
            } {
                Self::set(el, k, new);
            }
        }

        // Remove missing
        for (k, old_value) in old.iter() {
            if !new.contains_key(k) {
                Self::remove(el, k, old_value);
            }
        }
    }

    fn set(el: i32, key: &str, value: &AttributeOrProperty) {
        let string_value: &str = match value {
            AttributeOrProperty::Attribute(v) => v.as_ref(),
            AttributeOrProperty::Static(v) => v,
        };
        if let Err(err) = rust_wasm_binding::set_attribute(el, key, string_value) {
            tracing::warn!(?err, el, key, "failed to set attribute");
        }
    }

    fn remove(el: i32, key: &str, _old_value: &AttributeOrProperty) {
        if let Err(err) = rust_wasm_binding::remove_attribute(el, key) {
            tracing::warn!(?err, el, key, "failed to remove attribute");
        }
    }
}

impl Apply for Attributes {
    type Bundle = Self;
    type Element = i32;

    fn apply(self, _root: &BSubtree, el: i32) -> Self {
        match &self {
            Self::Static(arr) => {
                for (k, v) in arr.iter() {
                    Self::set(el, k, v);
                }
            }
            Self::Dynamic { keys, values } => {
                for (k, v) in keys.iter().zip(values.iter()) {
                    if let Some(v) = v {
                        Self::set(el, k, v)
                    }
                }
            }
            Self::IndexMap(m) => {
                for (k, v) in m.iter() {
                    Self::set(el, k, v)
                }
            }
        }
        self
    }

    fn apply_diff(self, _root: &BSubtree, el: i32, bundle: &mut Self) {
        #[inline]
        fn ptr_eq<T>(a: &[T], b: &[T]) -> bool {
            std::ptr::eq(a, b)
        }

        let ancestor = std::mem::replace(bundle, self);
        let bundle = &*bundle; // reborrow it immutably from here
        match (bundle, ancestor) {
            // Hot path
            (Self::Static(new), Self::Static(old)) if ptr_eq(new, old) => (),
            // Hot path
            (
                Self::Dynamic {
                    keys: new_k,
                    values: new_v,
                },
                Self::Dynamic {
                    keys: old_k,
                    values: old_v,
                },
            ) if ptr_eq(new_k, old_k) => {
                // Double zipping does not optimize well, so use asserts and unsafe instead
                assert_eq!(new_k.len(), new_v.len());
                assert_eq!(new_k.len(), old_v.len());
                for i in 0..new_k.len() {
                    macro_rules! key {
                        () => {
                            unsafe { new_k.get_unchecked(i) }
                        };
                    }
                    macro_rules! set {
                        ($new:expr) => {
                            Self::set(el, key!(), $new)
                        };
                    }

                    match unsafe { (new_v.get_unchecked(i), old_v.get_unchecked(i)) } {
                        (Some(new), Some(old)) => {
                            if new != old {
                                set!(new);
                            }
                        }
                        (Some(new), None) => set!(new),
                        (None, Some(old)) => {
                            Self::remove(el, key!(), old);
                        }
                        (None, None) => (),
                    }
                }
            }
            // For VTag's constructed outside the html! macro
            (Self::IndexMap(new), Self::IndexMap(ref old)) => {
                Self::apply_diff_index_maps(el, new, old);
            }
            // Cold path. Happens only with conditional swapping and reordering of `VTag`s with the
            // same tag and no keys.
            (new, ref ancestor) => {
                Self::apply_diff_as_maps(el, new, ancestor);
            }
        }
    }
}
