//! This module contains the bundle implementation of text [BText].

use gloo::utils::document;
use web_sys::{Element, Text as TextNode};

use super::{BNode, BSubtree, DomSlot, Reconcilable, ReconcileTarget};
use crate::html::AnyScope;
use crate::virtual_dom::{AttrValue, VText};

/// The bundle implementation to [VText]
pub(super) struct BText {
    text: AttrValue,
    text_node: TextNode,
}

impl ReconcileTarget for BText {
    fn detach(self, _root: &BSubtree, parent: &Element, parent_to_detach: bool) {
        if !parent_to_detach {
            let result = parent.remove_child(&self.text_node);

            if result.is_err() {
                tracing::warn!("Node not found to remove VText");
            }
        }
    }

    fn shift(&self, next_parent: &Element, slot: DomSlot) -> DomSlot {
        slot.insert(next_parent, &self.text_node);

        DomSlot::at(self.text_node.clone().into())
    }
}

impl Reconcilable for VText {
    type Bundle = BText;

    fn attach(
        self,
        _root: &BSubtree,
        _parent_scope: &AnyScope,
        parent: &Element,
        slot: DomSlot,
    ) -> (DomSlot, Self::Bundle) {
        let Self { text } = self;
        let text_node = document().create_text_node(&text);
        slot.insert(parent, &text_node);
        let node_ref = DomSlot::at(text_node.clone().into());
        (node_ref, BText { text, text_node })
    }

    /// Renders virtual node over existing `TextNode`, but only if value of text has changed.
    fn reconcile_node(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Element,
        slot: DomSlot,
        bundle: &mut BNode,
    ) -> DomSlot {
        match bundle {
            BNode::Text(btext) => self.reconcile(root, parent_scope, parent, slot, btext),
            _ => self.replace(root, parent_scope, parent, slot, bundle),
        }
    }

    fn reconcile(
        self,
        _root: &BSubtree,
        _parent_scope: &AnyScope,
        _parent: &Element,
        _slot: DomSlot,
        btext: &mut Self::Bundle,
    ) -> DomSlot {
        let Self { text } = self;
        let ancestor_text = std::mem::replace(&mut btext.text, text);
        if btext.text != ancestor_text {
            btext.text_node.set_node_value(Some(&btext.text));
        }
        DomSlot::at(btext.text_node.clone().into())
    }
}

impl std::fmt::Debug for BText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BText").field("text", &self.text).finish()
    }
}
