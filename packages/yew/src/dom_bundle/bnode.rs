//! This module contains the bundle version of an abstract node [BNode]

use std::fmt;
use std::rc::Rc;

use rust_wasm_binding::{Element, NodeOps};

use super::{BComp, BList, BPortal, BSubtree, BSuspense, BTag, BText, DomSlot};
use crate::dom_bundle::{Reconcilable, ReconcileTarget};
use crate::html::AnyScope;
use crate::utils::RcExt;
use crate::virtual_dom::{Key, VNode};

/// The bundle implementation to [VNode].
pub(super) enum BNode {
    /// A bind between `VTag` and an element node id.
    Tag(Box<BTag>),
    /// A bind between `VText` and a text node id.
    Text(BText),
    /// A bind between `VComp` and an element node id.
    Comp(BComp),
    /// A holder for a list of other nodes.
    List(BList),
    /// A portal to another part of the document
    Portal(BPortal),
    /// A holder for a raw, user-supplied DOM element node. The wrapper is
    /// shared via `Rc` so the bundle does not own the underlying slab id —
    /// the user code that constructed the [`VNode::VRef`] keeps its own
    /// clone.
    Ref(Rc<Element>),
    /// A suspendible document fragment.
    Suspense(Box<BSuspense>),
}

impl BNode {
    /// Get the key of the underlying node
    pub fn key(&self) -> Option<&Key> {
        match self {
            Self::Comp(bsusp) => bsusp.key(),
            Self::List(blist) => blist.key(),
            Self::Ref(_) => None,
            Self::Tag(btag) => btag.key(),
            Self::Text(_) => None,
            Self::Portal(bportal) => bportal.key(),
            Self::Suspense(bsusp) => bsusp.key(),
        }
    }
}

impl ReconcileTarget for BNode {
    /// Remove VNode from parent.
    fn detach(self, root: &BSubtree, parent: &Rc<Element>, parent_to_detach: bool) {
        match self {
            Self::Tag(vtag) => vtag.detach(root, parent, parent_to_detach),
            Self::Text(btext) => btext.detach(root, parent, parent_to_detach),
            Self::Comp(bsusp) => bsusp.detach(root, parent, parent_to_detach),
            Self::List(blist) => blist.detach(root, parent, parent_to_detach),
            Self::Ref(node) => {
                // Always remove user-defined nodes to clear possible parent references of them.
                // The `Rc<Element>` is *not* dropped here — the user code that constructed
                // the [`VNode::VRef`] still holds its own clone of the handle.
                if rust_wasm_binding::remove_child(parent.id(), node.id()).is_err() {
                    tracing::warn!("Node not found to remove VRef");
                }
            }
            Self::Portal(bportal) => bportal.detach(root, parent, parent_to_detach),
            Self::Suspense(bsusp) => bsusp.detach(root, parent, parent_to_detach),
        }
    }

    fn shift(&self, next_parent: &Rc<Element>, slot: DomSlot) -> DomSlot {
        match self {
            Self::Tag(ref vtag) => vtag.shift(next_parent, slot),
            Self::Text(ref btext) => btext.shift(next_parent, slot),
            Self::Comp(ref bsusp) => bsusp.shift(next_parent, slot),
            Self::List(ref vlist) => vlist.shift(next_parent, slot),
            Self::Ref(node) => {
                let id = node.id();
                slot.insert(next_parent, id);
                DomSlot::at(id)
            }
            Self::Portal(ref vportal) => vportal.shift(next_parent, slot),
            Self::Suspense(ref vsuspense) => vsuspense.shift(next_parent, slot),
        }
    }
}

impl Reconcilable for VNode {
    type Bundle = BNode;

    fn attach(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
    ) -> (DomSlot, Self::Bundle) {
        match self {
            VNode::VTag(vtag) => {
                let (node_ref, tag) =
                    RcExt::unwrap_or_clone(vtag).attach(root, parent_scope, parent, slot);
                (node_ref, tag.into())
            }
            VNode::VText(vtext) => {
                let (node_ref, text) = vtext.attach(root, parent_scope, parent, slot);
                (node_ref, text.into())
            }
            VNode::VComp(vcomp) => {
                let (node_ref, comp) =
                    RcExt::unwrap_or_clone(vcomp).attach(root, parent_scope, parent, slot);
                (node_ref, comp.into())
            }
            VNode::VList(vlist) => {
                let (node_ref, list) =
                    RcExt::unwrap_or_clone(vlist).attach(root, parent_scope, parent, slot);
                (node_ref, list.into())
            }
            VNode::VRef(node) => {
                let id = node.id();
                slot.insert(parent, id);
                (DomSlot::at(id), BNode::Ref(node))
            }
            VNode::VPortal(vportal) => {
                let (node_ref, portal) =
                    RcExt::unwrap_or_clone(vportal).attach(root, parent_scope, parent, slot);
                (node_ref, portal.into())
            }
            VNode::VSuspense(vsuspsense) => {
                let (node_ref, suspsense) =
                    RcExt::unwrap_or_clone(vsuspsense).attach(root, parent_scope, parent, slot);
                (node_ref, suspsense.into())
            }
        }
    }

    fn reconcile_node(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
        bundle: &mut BNode,
    ) -> DomSlot {
        self.reconcile(root, parent_scope, parent, slot, bundle)
    }

    fn reconcile(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
        bundle: &mut BNode,
    ) -> DomSlot {
        match self {
            VNode::VTag(vtag) => RcExt::unwrap_or_clone(vtag).reconcile_node(
                root,
                parent_scope,
                parent,
                slot,
                bundle,
            ),
            VNode::VText(vtext) => vtext.reconcile_node(root, parent_scope, parent, slot, bundle),
            VNode::VComp(vcomp) => RcExt::unwrap_or_clone(vcomp).reconcile_node(
                root,
                parent_scope,
                parent,
                slot,
                bundle,
            ),
            VNode::VList(vlist) => RcExt::unwrap_or_clone(vlist).reconcile_node(
                root,
                parent_scope,
                parent,
                slot,
                bundle,
            ),
            VNode::VRef(node) => match bundle {
                // Compare by slab id (identity). Two distinct `Rc<Element>`s
                // pointing at the same id should be treated as the same node.
                BNode::Ref(existing)
                    if Rc::ptr_eq(&node, existing) || node.id() == existing.id() =>
                {
                    DomSlot::at(node.id())
                }
                _ => VNode::VRef(node).replace(root, parent_scope, parent, slot, bundle),
            },
            VNode::VPortal(vportal) => RcExt::unwrap_or_clone(vportal).reconcile_node(
                root,
                parent_scope,
                parent,
                slot,
                bundle,
            ),
            VNode::VSuspense(vsuspsense) => RcExt::unwrap_or_clone(vsuspsense).reconcile_node(
                root,
                parent_scope,
                parent,
                slot,
                bundle,
            ),
        }
    }
}

impl From<BText> for BNode {
    #[inline]
    fn from(btext: BText) -> Self {
        Self::Text(btext)
    }
}

impl From<BList> for BNode {
    #[inline]
    fn from(blist: BList) -> Self {
        Self::List(blist)
    }
}

impl From<BTag> for BNode {
    #[inline]
    fn from(btag: BTag) -> Self {
        Self::Tag(Box::new(btag))
    }
}

impl From<BComp> for BNode {
    #[inline]
    fn from(bcomp: BComp) -> Self {
        Self::Comp(bcomp)
    }
}

impl From<BPortal> for BNode {
    #[inline]
    fn from(bportal: BPortal) -> Self {
        Self::Portal(bportal)
    }
}

impl From<BSuspense> for BNode {
    #[inline]
    fn from(bsusp: BSuspense) -> Self {
        Self::Suspense(Box::new(bsusp))
    }
}

impl fmt::Debug for BNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Tag(ref vtag) => vtag.fmt(f),
            Self::Text(ref btext) => btext.fmt(f),
            Self::Comp(ref bsusp) => bsusp.fmt(f),
            Self::List(ref vlist) => vlist.fmt(f),
            Self::Ref(ref vref) => write!(f, "VRef({})", vref.id()),
            Self::Portal(ref vportal) => vportal.fmt(f),
            Self::Suspense(ref bsusp) => bsusp.fmt(f),
        }
    }
}
