//! This module contains the bundle implementation of a tag [BTag]

mod attributes;
mod listeners;

use std::hint::unreachable_unchecked;
use std::ops::DerefMut;

use listeners::ListenerRegistration;
pub use listeners::Registry;

use super::{BNode, BSubtree, DomSlot, Reconcilable, ReconcileTarget};
use crate::html::AnyScope;
use crate::virtual_dom::vtag::{
    InputFields, TextareaFields, TextareaMarker, VTagInner, Value, MATHML_NAMESPACE, SVG_NAMESPACE,
};
use crate::virtual_dom::{AttrValue, Attributes, Key, VTag};
use crate::NodeRef;

/// Applies contained changes to an element identified by a Paws node id.
trait Apply {
    /// Opaque handle to the element the change is applied to. In the Paws
    /// fork this is always `i32` — the marker type is kept so the existing
    /// `Value<T>` / `InputFields` / `TextareaFields` impls can stay generic.
    type Element;
    type Bundle;

    /// Apply contained values to [Element](Self::Element) with no ancestor
    fn apply(self, root: &BSubtree, el: &Self::Element) -> Self::Bundle;

    /// Apply diff between [self] and `bundle` to [Element](Self::Element).
    fn apply_diff(self, root: &BSubtree, el: &Self::Element, bundle: &mut Self::Bundle);
}

/// [BTag] fields that are specific to different [BTag] kinds.
/// Decreases the memory footprint of [BTag] by avoiding impossible field and value combinations.
#[derive(Debug)]
enum BTagInner {
    /// Fields specific to `<input>` elements.
    Input(InputFields),
    /// Fields specific to `<textarea>` elements.
    Textarea {
        /// Contains a value of a `<textarea>` element.
        value: Value<TextareaMarker>,
    },
    /// Fields for all other kinds of [VTag]s
    Other {
        /// A tag of the element.
        tag: AttrValue,
        /// Child node.
        child_bundle: BNode,
    },
}

/// The bundle implementation to [VTag]
#[derive(Debug)]
pub(super) struct BTag {
    /// [BTag] fields that are specific to different [BTag] kinds.
    inner: BTagInner,
    listeners: ListenerRegistration,
    attributes: Attributes,
    /// Paws node id for the created element.
    reference: i32,
    /// A node reference used for DOM access in Component lifecycle methods
    node_ref: NodeRef,
    key: Option<Key>,
}

impl ReconcileTarget for BTag {
    fn detach(self, root: &BSubtree, parent: i32, parent_to_detach: bool) {
        self.listeners.unregister(root);

        let node = self.reference;
        // recursively remove its children
        if let BTagInner::Other { child_bundle, .. } = self.inner {
            // This tag will be removed, so there's no point to remove any child.
            child_bundle.detach(root, node, true);
        }
        if !parent_to_detach {
            let result = rust_wasm_binding::remove_child(parent, node);

            if result.is_err() {
                tracing::warn!("Node not found to remove VTag");
            }
        }
        // It could be that the ref was already reused when rendering another element.
        // Only unset the ref if it still belongs to our node
        if self.node_ref.get() == Some(node) {
            self.node_ref.set(None);
        }
    }

    fn shift(&self, next_parent: i32, slot: DomSlot) -> DomSlot {
        slot.insert(next_parent, self.reference);

        DomSlot::at(self.reference)
    }
}

impl Reconcilable for VTag {
    type Bundle = BTag;

    fn attach(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: i32,
        slot: DomSlot,
    ) -> (DomSlot, Self::Bundle) {
        let el = self.create_element(parent);
        let Self {
            listeners,
            attributes,
            node_ref,
            key,
            ..
        } = self;

        // Apply attributes BEFORE inserting the element into the DOM
        // This is crucial for SVG animation elements where the animation
        // starts immediately upon DOM insertion
        let attributes = attributes.apply(root, &el);
        let listeners = listeners.apply(root, &el);

        // Now insert the element with attributes already set
        slot.insert(parent, el);

        let inner = match self.inner {
            VTagInner::Input(f) => {
                let f = f.apply(root, &el);
                BTagInner::Input(f)
            }
            VTagInner::Textarea(f) => {
                let value = f.apply(root, &el);
                BTagInner::Textarea { value }
            }
            VTagInner::Other { children, tag } => {
                let (_, child_bundle) = children.attach(root, parent_scope, el, DomSlot::at_end());
                BTagInner::Other { child_bundle, tag }
            }
        };
        node_ref.set(Some(el));
        (
            DomSlot::at(el),
            BTag {
                inner,
                listeners,
                reference: el,
                attributes,
                key,
                node_ref,
            },
        )
    }

    fn reconcile_node(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: i32,
        slot: DomSlot,
        bundle: &mut BNode,
    ) -> DomSlot {
        // This kind of branching patching routine reduces branch predictor misses and the need to
        // unpack the enums (including `Option`s) all the time, resulting in a more streamlined
        // patching flow
        match bundle {
            // If the ancestor is a tag of the same type, don't recreate, keep the
            // old tag and update its attributes and children.
            BNode::Tag(ex)
                if self.key == ex.key
                    && match (&self.inner, &ex.inner) {
                        (VTagInner::Input(_), BTagInner::Input(_)) => true,
                        (VTagInner::Textarea { .. }, BTagInner::Textarea { .. }) => true,
                        (VTagInner::Other { tag: l, .. }, BTagInner::Other { tag: r, .. })
                            if l == r =>
                        {
                            true
                        }
                        _ => false,
                    } =>
            {
                return self.reconcile(root, parent_scope, parent, slot, ex.deref_mut());
            }
            _ => {}
        };
        self.replace(root, parent_scope, parent, slot, bundle)
    }

    fn reconcile(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        _parent: i32,
        _slot: DomSlot,
        tag: &mut Self::Bundle,
    ) -> DomSlot {
        let el = tag.reference;
        self.attributes.apply_diff(root, &el, &mut tag.attributes);
        self.listeners.apply_diff(root, &el, &mut tag.listeners);

        match (self.inner, &mut tag.inner) {
            (VTagInner::Input(new), BTagInner::Input(old)) => {
                new.apply_diff(root, &el, old);
            }
            (
                VTagInner::Textarea(TextareaFields { value: new, .. }),
                BTagInner::Textarea { value: old },
            ) => {
                new.apply_diff(root, &el, old);
            }
            (
                VTagInner::Other { children: new, .. },
                BTagInner::Other {
                    child_bundle: old, ..
                },
            ) => {
                new.reconcile(root, parent_scope, el, DomSlot::at_end(), old);
            }
            // Can not happen, because we checked for tag equability above
            _ => unsafe { unreachable_unchecked() },
        }

        tag.key = self.key;

        if self.node_ref != tag.node_ref && tag.node_ref.get() == Some(el) {
            tag.node_ref.set(None);
        }
        if self.node_ref != tag.node_ref {
            tag.node_ref = self.node_ref;
            tag.node_ref.set(Some(el));
        }

        DomSlot::at(el)
    }
}

impl VTag {
    /// Create the host-side DOM element, choosing the SVG/MathML/HTML
    /// namespace based on the tag name, the parent's namespace, and an
    /// optional `xmlns` attribute.
    fn create_element(&self, parent: i32) -> i32 {
        let tag = self.tag();
        if let Some(xmlns) = self
            .attributes
            .iter()
            .find(|(k, _)| *k == "xmlns")
            .map(|(_, v)| v)
        {
            rust_wasm_binding::create_element_ns(xmlns, tag)
                .expect("can't create namespaced element for vtag")
        } else if tag == "svg" || parent_namespace_is(parent, SVG_NAMESPACE) {
            rust_wasm_binding::create_element_ns(SVG_NAMESPACE, tag)
                .expect("can't create namespaced element for vtag")
        } else if tag == "math" || parent_namespace_is(parent, MATHML_NAMESPACE) {
            rust_wasm_binding::create_element_ns(MATHML_NAMESPACE, tag)
                .expect("can't create namespaced element for vtag")
        } else {
            rust_wasm_binding::create_element(tag).expect("can't create element for vtag")
        }
    }
}

fn parent_namespace_is(parent: i32, expected: &str) -> bool {
    // Reasonably-sized local buffer covering every known HTML / SVG / MathML
    // namespace URI. The host returns the required length; if it exceeds the
    // buffer we conservatively assume "not a match" (namespace detection
    // never invents SVG context where there isn't one).
    let mut buf = [0u8; 128];
    match rust_wasm_binding::get_namespace_uri(parent, &mut buf) {
        Ok(Some(len)) if len <= buf.len() => &buf[..len] == expected.as_bytes(),
        _ => false,
    }
}

impl BTag {
    /// Get the key of the underlying tag
    pub fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }
}
#[cfg(test)]
mod tests_without_browser {
    use crate::html;
    use crate::virtual_dom::VNode;

    #[test]
    fn html_if_bool() {
        assert_eq!(
            html! {
                if true {
                    <div class="foo" />
                }
            },
            html! {
                <>
                    <div class="foo" />
                </>
            },
        );
        assert_eq!(
            html! {
                if false {
                    <div class="foo" />
                } else {
                    <div class="bar" />
                }
            },
            html! {
                <><div class="bar" /></>
            },
        );
        assert_eq!(
            html! {
                if false {
                    <div class="foo" />
                }
            },
            html! {
                <></>
            },
        );

        // non-root tests
        assert_eq!(
            html! {
                <div>
                    if true {
                        <div class="foo" />
                    }
                </div>
            },
            html! {
                <div>
                    <><div class="foo" /></>
                </div>
            },
        );
        assert_eq!(
            html! {
                <div>
                    if false {
                        <div class="foo" />
                    } else {
                        <div class="bar" />
                    }
                </div>
            },
            html! {
                <div>
                    <><div class="bar" /></>
                </div>
            },
        );
        assert_eq!(
            html! {
                <div>
                    if false {
                        <div class="foo" />
                    }
                </div>
            },
            html! {
                <div>
                    <></>
                </div>
            },
        );
    }

    #[test]
    fn html_if_option() {
        let option_foo = Some("foo");
        let none: Option<&'static str> = None;
        assert_eq!(
            html! {
                if let Some(class) = option_foo {
                    <div class={class} />
                }
            },
            html! {
                <>
                    <div class={Some("foo")} />
                </>
            },
        );
        assert_eq!(
            html! {
                if let Some(class) = none {
                    <div class={class} />
                } else {
                    <div class="bar" />
                }
            },
            html! {
                <>
                    <div class="bar" />
                </>
            },
        );
        assert_eq!(
            html! {
                if let Some(class) = none {
                    <div class={class} />
                }
            },
            html! {
                <></>
            },
        );

        // non-root tests
        assert_eq!(
            html! {
                <div>
                    if let Some(class) = option_foo {
                        <div class={class} />
                    }
                </div>
            },
            html! {
                <div>
                    <>
                        <div class={Some("foo")} />
                    </>
                </div>
            },
        );
        assert_eq!(
            html! {
                <div>
                    if let Some(class) = none {
                        <div class={class} />
                    } else {
                        <div class="bar" />
                    }
                </div>
            },
            html! {
                <div>
                    <>
                        <div class="bar" />
                    </>
                </div>
            },
        );
        assert_eq!(
            html! {
                <div>
                    if let Some(class) = none {
                        <div class={class} />
                    }
                </div>
            },
            html! { <div><></></div> },
        );
    }

    #[test]
    fn input_checked_stays_there() {
        let tag = html! {
            <input checked={true} />
        };
        match tag {
            VNode::VTag(tag) => {
                assert_eq!(tag.checked(), Some(true));
            }
            _ => unreachable!(),
        }
    }
    #[test]
    fn non_input_checked_stays_there() {
        let tag = html! {
            <my-el checked="true" />
        };
        match tag {
            VNode::VTag(tag) => {
                assert_eq!(
                    tag.attributes.iter().find(|(k, _)| *k == "checked"),
                    Some(("checked", "true"))
                );
            }
            _ => unreachable!(),
        }
    }
}
