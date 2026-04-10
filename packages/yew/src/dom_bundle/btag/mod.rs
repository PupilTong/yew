//! This module contains the bundle implementation of a tag [BTag]

mod attributes;
mod listeners;

use std::hint::unreachable_unchecked;
use std::ops::DerefMut;
use std::rc::Rc;

use listeners::ListenerRegistration;
pub use listeners::Registry;
use rust_wasm_binding::{Element, NodeOps};

use super::{BNode, BSubtree, DomSlot, Reconcilable, ReconcileTarget};
use crate::html::AnyScope;
use crate::virtual_dom::vtag::{
    InputFields, TextareaFields, TextareaMarker, VTagInner, Value, MATHML_NAMESPACE, SVG_NAMESPACE,
};
use crate::virtual_dom::{AttrValue, Attributes, Key, VTag};
use crate::NodeRef;

/// Applies contained changes to an element via a borrowed wrapper.
trait Apply {
    type Bundle;

    /// Apply contained values to `el` with no ancestor.
    fn apply(self, root: &BSubtree, el: &Element) -> Self::Bundle;

    /// Apply diff between [self] and `bundle` to `el`.
    fn apply_diff(self, root: &BSubtree, el: &Element, bundle: &mut Self::Bundle);
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
    /// RAII handle for the created element node. Wrapped in `Rc` so child
    /// components / bundles can pass `&self.reference` (an `&Rc<Element>`)
    /// down their attach/reconcile chains as the parent. The last drop of
    /// the `Rc` releases the host-side slab slot.
    reference: Rc<Element>,
    /// A node reference used for DOM access in Component lifecycle methods
    node_ref: NodeRef,
    key: Option<Key>,
}

impl ReconcileTarget for BTag {
    fn detach(self, root: &BSubtree, parent: &Rc<Element>, parent_to_detach: bool) {
        let Self {
            inner,
            listeners,
            attributes: _,
            reference,
            node_ref,
            key: _,
        } = self;

        listeners.unregister(root);
        let node_id = reference.id();

        // Recursively detach children FIRST so listeners are cleaned up
        // before the host-side cascade.
        if let BTagInner::Other { child_bundle, .. } = inner {
            child_bundle.detach(root, &reference, true);
        }

        if !parent_to_detach {
            // Physically detach from the parent. The reference's last
            // `Rc<Element>` clone (this one, plus any held by children
            // components that have already been destroyed) eventually
            // calls `destroy_element` to release the slab entry. The
            // destructor swallows errors silently, so a host-side cascade
            // (when `parent_to_detach == true`) won't double-fire.
            if let Err(err) = rust_wasm_binding::remove_child(parent.id(), node_id) {
                tracing::warn!(?err, "Node not found to remove VTag");
            }
        }
        // `reference` drops at the end of this scope. If it is the last
        // surviving `Rc<Element>` clone the inner `Element`'s destructor
        // calls `destroy_element`; any earlier host-side cascade has
        // already removed the slab entry and the destroy call is a no-op.

        // It could be that the ref was already reused when rendering another element.
        // Only unset the ref if it still belongs to our node.
        if node_ref.get().map(|e| e.id()) == Some(node_id) {
            node_ref.set(None);
        }
    }

    fn shift(&self, next_parent: &Rc<Element>, slot: DomSlot) -> DomSlot {
        let el_id = self.reference.id();
        slot.insert(next_parent, el_id);
        DomSlot::at(el_id)
    }
}

impl Reconcilable for VTag {
    type Bundle = BTag;

    fn attach(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
    ) -> (DomSlot, Self::Bundle) {
        let reference = self.create_element(parent);
        let el_id = reference.id();
        let Self {
            listeners,
            attributes,
            node_ref,
            key,
            inner: self_inner,
            ..
        } = self;

        // Apply attributes BEFORE inserting the element into the DOM.
        // This is crucial for SVG animation elements where the animation
        // starts immediately upon DOM insertion.
        let attributes = attributes.apply(root, &reference);
        let listeners = listeners.apply(root, &reference);

        // Now insert the element with attributes already set.
        slot.insert(parent, el_id);

        let inner = match self_inner {
            VTagInner::Input(f) => {
                let f = f.apply(root, &reference);
                BTagInner::Input(f)
            }
            VTagInner::Textarea(f) => {
                let value = f.apply(root, &reference);
                BTagInner::Textarea { value }
            }
            VTagInner::Other { children, tag } => {
                let (_, child_bundle) =
                    children.attach(root, parent_scope, &reference, DomSlot::at_end());
                BTagInner::Other { child_bundle, tag }
            }
        };
        node_ref.set(Some(Rc::clone(&reference)));
        (
            DomSlot::at(el_id),
            BTag {
                inner,
                listeners,
                reference,
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
        parent: &Rc<Element>,
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
        _parent: &Rc<Element>,
        _slot: DomSlot,
        tag: &mut Self::Bundle,
    ) -> DomSlot {
        let el_id = tag.reference.id();
        self.attributes
            .apply_diff(root, &tag.reference, &mut tag.attributes);
        self.listeners
            .apply_diff(root, &tag.reference, &mut tag.listeners);

        match (self.inner, &mut tag.inner) {
            (VTagInner::Input(new), BTagInner::Input(old)) => {
                new.apply_diff(root, &tag.reference, old);
            }
            (
                VTagInner::Textarea(TextareaFields { value: new, .. }),
                BTagInner::Textarea { value: old },
            ) => {
                new.apply_diff(root, &tag.reference, old);
            }
            (
                VTagInner::Other { children: new, .. },
                BTagInner::Other {
                    child_bundle: old, ..
                },
            ) => {
                new.reconcile(root, parent_scope, &tag.reference, DomSlot::at_end(), old);
            }
            // Can not happen, because we checked for tag equability above
            _ => unsafe { unreachable_unchecked() },
        }

        tag.key = self.key;

        if self.node_ref != tag.node_ref && tag.node_ref.get().map(|e| e.id()) == Some(el_id) {
            tag.node_ref.set(None);
        }
        if self.node_ref != tag.node_ref {
            tag.node_ref = self.node_ref;
            tag.node_ref.set(Some(Rc::clone(&tag.reference)));
        }

        DomSlot::at(el_id)
    }
}

impl VTag {
    /// Create the host-side DOM element, choosing the SVG/MathML/HTML
    /// namespace based on the tag name, the parent's namespace, and an
    /// optional `xmlns` attribute. Returns an `Rc` so the new element can
    /// be both stored in the [BTag] and passed to children as a parent.
    fn create_element(&self, parent: &Element) -> Rc<Element> {
        let tag = self.tag();
        let element = if let Some(xmlns) = self
            .attributes
            .iter()
            .find(|(k, _)| *k == "xmlns")
            .map(|(_, v)| v)
        {
            Element::new_ns(xmlns, tag).expect("can't create namespaced element for vtag")
        } else if tag == "svg" || parent_namespace_is(parent, SVG_NAMESPACE) {
            Element::new_ns(SVG_NAMESPACE, tag).expect("can't create namespaced element for vtag")
        } else if tag == "math" || parent_namespace_is(parent, MATHML_NAMESPACE) {
            Element::new_ns(MATHML_NAMESPACE, tag)
                .expect("can't create namespaced element for vtag")
        } else {
            Element::new(tag).expect("can't create element for vtag")
        };
        Rc::new(element)
    }
}

fn parent_namespace_is(parent: &Element, expected: &str) -> bool {
    // Reasonably-sized local buffer covering every known HTML / SVG / MathML
    // namespace URI. The host returns the required length; if it exceeds the
    // buffer we conservatively assume "not a match" (namespace detection
    // never invents SVG context where there isn't one).
    let mut buf = [0u8; 128];
    match rust_wasm_binding::get_namespace_uri(parent.id(), &mut buf) {
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
