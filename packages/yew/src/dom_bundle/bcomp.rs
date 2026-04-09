//! This module contains the bundle implementation of a virtual component [BComp].

use std::any::TypeId;
use std::borrow::Borrow;
use std::fmt;

use web_sys::Element;

use super::{BNode, BSubtree, DomSlot, DynamicDomSlot, Reconcilable, ReconcileTarget};
use crate::html::{AnyScope, Scoped};
use crate::virtual_dom::{Key, VComp};

/// A virtual component. Compare with [VComp].
pub(super) struct BComp {
    type_id: TypeId,
    scope: Box<dyn Scoped>,
    /// An internal [`DomSlot`] passed around to track this components position. This
    /// will dynamically adjust when a lifecycle changes the render state of this component.
    own_position: DynamicDomSlot,
    key: Option<Key>,
}

impl BComp {
    /// Get the key of the underlying component
    pub fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }
}

impl fmt::Debug for BComp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BComp")
            .field("root", &self.scope.as_ref().render_state())
            .finish()
    }
}

impl ReconcileTarget for BComp {
    fn detach(self, _root: &BSubtree, _parent: &Element, parent_to_detach: bool) {
        self.scope.destroy_boxed(parent_to_detach);
    }

    fn shift(&self, next_parent: &Element, slot: DomSlot) -> DomSlot {
        self.scope.shift_node(next_parent.clone(), slot);

        self.own_position.to_position()
    }
}

impl Reconcilable for VComp {
    type Bundle = BComp;

    fn attach(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Element,
        slot: DomSlot,
    ) -> (DomSlot, Self::Bundle) {
        let VComp {
            type_id,
            mountable,
            key,
            ..
        } = self;

        let (scope, internal_ref) = mountable.mount(root, parent_scope, parent.to_owned(), slot);

        (
            internal_ref.to_position(),
            BComp {
                type_id,
                scope,
                own_position: internal_ref,
                key,
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
        match bundle {
            // If the existing bundle is the same type, reuse it and update its properties
            BNode::Comp(ref mut bcomp)
                if self.type_id == bcomp.type_id && self.key == bcomp.key =>
            {
                self.reconcile(root, parent_scope, parent, slot, bcomp)
            }
            _ => self.replace(root, parent_scope, parent, slot, bundle),
        }
    }

    fn reconcile(
        self,
        _root: &BSubtree,
        _parent_scope: &AnyScope,
        _parent: &Element,
        slot: DomSlot,
        bcomp: &mut Self::Bundle,
    ) -> DomSlot {
        let VComp { mountable, key, .. } = self;

        bcomp.key = key;
        mountable.reuse(bcomp.scope.borrow(), slot);
        bcomp.own_position.to_position()
    }
}
