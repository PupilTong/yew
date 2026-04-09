//! This module contains the bundle implementation of text [BText].

use super::{BNode, BSubtree, DomSlot, Reconcilable, ReconcileTarget};
use crate::html::AnyScope;
use crate::virtual_dom::{AttrValue, VText};

/// The bundle implementation to [VText]
pub(super) struct BText {
    text: AttrValue,
    /// Paws node id for the host-side text node.
    text_node: i32,
}

impl ReconcileTarget for BText {
    fn detach(self, _root: &BSubtree, parent: i32, parent_to_detach: bool) {
        if !parent_to_detach {
            let result = rust_wasm_binding::remove_child(parent, self.text_node);

            if result.is_err() {
                tracing::warn!("Node not found to remove VText");
            }
        }
    }

    fn shift(&self, next_parent: i32, slot: DomSlot) -> DomSlot {
        slot.insert(next_parent, self.text_node);

        DomSlot::at(self.text_node)
    }
}

impl Reconcilable for VText {
    type Bundle = BText;

    fn attach(
        self,
        _root: &BSubtree,
        _parent_scope: &AnyScope,
        parent: i32,
        slot: DomSlot,
    ) -> (DomSlot, Self::Bundle) {
        let Self { text } = self;
        let text_node =
            rust_wasm_binding::create_text_node(&text).expect("failed to create text node");
        slot.insert(parent, text_node);
        let node_ref = DomSlot::at(text_node);
        (node_ref, BText { text, text_node })
    }

    /// Renders virtual node over existing text node, but only if value of text has changed.
    fn reconcile_node(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: i32,
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
        _parent: i32,
        _slot: DomSlot,
        btext: &mut Self::Bundle,
    ) -> DomSlot {
        let Self { text } = self;
        let ancestor_text = std::mem::replace(&mut btext.text, text);
        if btext.text != ancestor_text {
            rust_wasm_binding::set_node_value(btext.text_node, &btext.text)
                .expect("failed to set text node value");
        }
        DomSlot::at(btext.text_node)
    }
}

impl std::fmt::Debug for BText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BText").field("text", &self.text).finish()
    }
}
