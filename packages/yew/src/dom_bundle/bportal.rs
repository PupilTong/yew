//! This module contains the bundle implementation of a portal [BPortal].

use std::rc::Rc;

use lynx_sys::{Element, ExternRef};

use super::{test_log, BNode, BSubtree, DomSlot};
use crate::dom_bundle::{Reconcilable, ReconcileTarget};
use crate::html::AnyScope;
use crate::virtual_dom::{Key, VPortal};

/// The bundle implementation to [VPortal].
#[derive(Debug)]
pub struct BPortal {
    // The inner root
    inner_root: BSubtree,
    /// The element under which the portal content is inserted. Held as
    /// `Rc<Element>` so the user code can keep its own clone of the host
    /// alive while yew renders into it.
    host: Rc<Element>,
    /// The next sibling after the inserted content.
    inner_sibling: Option<ExternRef>,
    /// The inserted node
    node: Box<BNode>,
}

impl ReconcileTarget for BPortal {
    fn detach(self, _root: &BSubtree, _parent: &Rc<Element>, _parent_to_detach: bool) {
        test_log!("Detaching portal from host",);
        self.node.detach(&self.inner_root, &self.host, false);
    }

    fn shift(&self, _next_parent: &Rc<Element>, slot: DomSlot) -> DomSlot {
        // portals have nothing in its original place of DOM, we also do nothing.
        slot
    }
}

impl Reconcilable for VPortal {
    type Bundle = BPortal;

    fn attach(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        host_slot: DomSlot,
    ) -> (DomSlot, Self::Bundle) {
        let Self {
            host,
            inner_sibling,
            node,
        } = self;
        let inner_slot = DomSlot::create(inner_sibling);
        let inner_root = root.create_subroot(parent, &host);
        let (_, inner) = node.attach(&inner_root, parent_scope, &host, inner_slot);
        (
            host_slot,
            BPortal {
                inner_root,
                host,
                node: Box::new(inner),
                inner_sibling,
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
        match bundle {
            BNode::Portal(portal) => self.reconcile(root, parent_scope, parent, slot, portal),
            _ => self.replace(root, parent_scope, parent, slot, bundle),
        }
    }

    fn reconcile(
        self,
        _root: &BSubtree,
        parent_scope: &AnyScope,
        _parent: &Rc<Element>,
        host_slot: DomSlot,
        portal: &mut Self::Bundle,
    ) -> DomSlot {
        let Self {
            host,
            inner_sibling,
            node,
        } = self;

        let old_host_id = portal.host.id();
        let new_host_id = host.id();
        portal.host = host;

        let should_shift = old_host_id != new_host_id || portal.inner_sibling != inner_sibling;
        portal.inner_sibling = inner_sibling;
        let inner_slot = DomSlot::create(portal.inner_sibling);

        if should_shift {
            // Remount the inner node somewhere else instead of diffing
            // Move the node, but keep the state
            portal.node.shift(&portal.host, inner_slot.clone());
        }
        node.reconcile_node(
            &portal.inner_root,
            parent_scope,
            &portal.host,
            inner_slot,
            &mut portal.node,
        );
        host_slot
    }
}

impl BPortal {
    /// Get the key of the underlying portal
    pub fn key(&self) -> Option<&Key> {
        self.node.key()
    }
}
