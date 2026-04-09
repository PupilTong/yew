//! This module contains the bundle implementation of text [BText].

use rust_wasm_binding::{NodeOps, Text};

use super::{BNode, BSubtree, DomSlot, Reconcilable, ReconcileTarget};
use crate::html::AnyScope;
use crate::virtual_dom::{AttrValue, VText};

/// The bundle implementation to [VText]
pub(super) struct BText {
    text: AttrValue,
    /// RAII wrapper for the host-side text node. Dropping destroys it.
    text_node: Text,
}

impl ReconcileTarget for BText {
    fn detach(self, _root: &BSubtree, parent: i32, parent_to_detach: bool) {
        let Self { text: _, text_node } = self;
        let node_id = text_node.id();

        if parent_to_detach {
            // Parent is going away; host will cascade the destroy. Forget
            // the wrapper to avoid double-destroying the already-removed
            // slab entry.
            let _ = text_node.into_raw();
        } else {
            // Physically detach from parent. Drop (below) will destroy
            // the slab entry.
            if let Err(err) = rust_wasm_binding::remove_child(parent, node_id) {
                tracing::warn!(?err, "Node not found to remove VText");
            }
            // text_node dropped here → destroy_element
        }
    }

    fn shift(&self, next_parent: i32, slot: DomSlot) -> DomSlot {
        let node_id = self.text_node.id();
        slot.insert(next_parent, node_id);
        DomSlot::at(node_id)
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
        let text_node = Text::new(&text).expect("failed to create text node");
        let node_id = text_node.id();
        slot.insert(parent, node_id);
        let node_ref = DomSlot::at(node_id);
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
            btext
                .text_node
                .set_node_value(&btext.text)
                .expect("failed to set text node value");
        }
        DomSlot::at(btext.text_node.id())
    }
}

impl std::fmt::Debug for BText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BText").field("text", &self.text).finish()
    }
}
