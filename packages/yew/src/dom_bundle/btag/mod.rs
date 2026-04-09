//! This module contains the bundle implementation of a tag [BTag]

mod attributes;
mod listeners;

use std::cell::RefCell;
use std::collections::HashMap;
use std::hint::unreachable_unchecked;
use std::ops::DerefMut;

use gloo::utils::document;
use listeners::ListenerRegistration;
pub use listeners::Registry;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlTextAreaElement as TextAreaElement};

use super::{BNode, BSubtree, DomSlot, Reconcilable, ReconcileTarget};
use crate::html::AnyScope;
use crate::virtual_dom::vtag::{
    InputFields, TextareaFields, VTagInner, Value, MATHML_NAMESPACE, SVG_NAMESPACE,
};
use crate::virtual_dom::{AttrValue, Attributes, Key, VTag};
use crate::NodeRef;

/// Applies contained changes to DOM [web_sys::Element]
trait Apply {
    /// [web_sys::Element] subtype to apply the changes to
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
    /// Fields specific to
    /// [InputElement](https://developer.mozilla.org/en-US/docs/Web/HTML/Element/input)
    Input(InputFields),
    /// Fields specific to
    /// [TextArea](https://developer.mozilla.org/en-US/docs/Web/HTML/Element/textarea)
    Textarea {
        /// Contains a value of an
        /// [TextArea](https://developer.mozilla.org/en-US/docs/Web/HTML/Element/textarea)
        value: Value<TextAreaElement>,
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
    /// A reference to the DOM [`Element`].
    reference: Element,
    /// A node reference used for DOM access in Component lifecycle methods
    node_ref: NodeRef,
    key: Option<Key>,
}

impl ReconcileTarget for BTag {
    fn detach(self, root: &BSubtree, parent: &Element, parent_to_detach: bool) {
        self.listeners.unregister(root);

        let node = self.reference;
        // recursively remove its children
        if let BTagInner::Other { child_bundle, .. } = self.inner {
            // This tag will be removed, so there's no point to remove any child.
            child_bundle.detach(root, &node, true);
        }
        if !parent_to_detach {
            let result = parent.remove_child(&node);

            if result.is_err() {
                tracing::warn!("Node not found to remove VTag");
            }
        }
        // It could be that the ref was already reused when rendering another element.
        // Only unset the ref it still belongs to our node
        if self.node_ref.get().as_ref() == Some(&node) {
            self.node_ref.set(None);
        }
    }

    fn shift(&self, next_parent: &Element, slot: DomSlot) -> DomSlot {
        slot.insert(next_parent, &self.reference);

        DomSlot::at(self.reference.clone().into())
    }
}

impl Reconcilable for VTag {
    type Bundle = BTag;

    fn attach(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Element,
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
        slot.insert(parent, &el);

        let inner = match self.inner {
            VTagInner::Input(f) => {
                let f = f.apply(root, el.unchecked_ref());
                BTagInner::Input(f)
            }
            VTagInner::Textarea(f) => {
                let value = f.apply(root, el.unchecked_ref());
                BTagInner::Textarea { value }
            }
            VTagInner::Other { children, tag } => {
                let (_, child_bundle) = children.attach(root, parent_scope, &el, DomSlot::at_end());
                BTagInner::Other { child_bundle, tag }
            }
        };
        node_ref.set(Some(el.clone().into()));
        (
            DomSlot::at(el.clone().into()),
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
        parent: &Element,
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
        _parent: &Element,
        _slot: DomSlot,
        tag: &mut Self::Bundle,
    ) -> DomSlot {
        let el = &tag.reference;
        self.attributes.apply_diff(root, el, &mut tag.attributes);
        self.listeners.apply_diff(root, el, &mut tag.listeners);

        match (self.inner, &mut tag.inner) {
            (VTagInner::Input(new), BTagInner::Input(old)) => {
                new.apply_diff(root, el.unchecked_ref(), old);
            }
            (
                VTagInner::Textarea(TextareaFields { value: new, .. }),
                BTagInner::Textarea { value: old },
            ) => {
                new.apply_diff(root, el.unchecked_ref(), old);
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

        if self.node_ref != tag.node_ref && tag.node_ref.get().as_ref() == Some(el) {
            tag.node_ref.set(None);
        }
        if self.node_ref != tag.node_ref {
            tag.node_ref = self.node_ref;
            tag.node_ref.set(Some(el.clone().into()));
        }

        DomSlot::at(el.clone().into())
    }
}

impl VTag {
    fn create_element(&self, parent: &Element) -> Element {
        let tag = self.tag();
        // check for an xmlns attribute. If it exists, create an element with the specified
        // namespace
        if let Some(xmlns) = self
            .attributes
            .iter()
            .find(|(k, _)| *k == "xmlns")
            .map(|(_, v)| v)
        {
            document()
                .create_element_ns(Some(xmlns), tag)
                .expect("can't create namespaced element for vtag")
        } else if tag == "svg" || parent.namespace_uri().is_some_and(|ns| ns == SVG_NAMESPACE) {
            let namespace = Some(SVG_NAMESPACE);
            document()
                .create_element_ns(namespace, tag)
                .expect("can't create namespaced element for vtag")
        } else if tag == "math"
            || parent
                .namespace_uri()
                .is_some_and(|ns| ns == MATHML_NAMESPACE)
        {
            let namespace = Some(MATHML_NAMESPACE);
            document()
                .create_element_ns(namespace, tag)
                .expect("can't create namespaced element for vtag")
        } else {
            thread_local! {
                static CACHED_ELEMENTS: RefCell<HashMap<String, Element>> = RefCell::new(HashMap::with_capacity(32));
            }

            CACHED_ELEMENTS.with(|cache| {
                let mut cache = cache.borrow_mut();
                let cached = cache.get(tag).map(|el| {
                    el.clone_node()
                        .expect("couldn't clone cached element")
                        .unchecked_into::<Element>()
                });
                cached.unwrap_or_else(|| {
                    let to_be_cached = document()
                        .create_element(tag)
                        .expect("can't create element for vtag");
                    cache.insert(
                        tag.to_string(),
                        to_be_cached
                            .clone_node()
                            .expect("couldn't clone node to be cached")
                            .unchecked_into(),
                    );
                    to_be_cached
                })
            })
        }
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
