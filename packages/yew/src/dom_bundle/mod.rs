//! Realizing a virtual dom on the actual DOM
//!
//! A bundle, borrowed from the mathematical meaning, is any structure over some base space.
//! In our case, the base space is the virtual dom we're trying to render.
//! In order to efficiently implement updates, and diffing, additional information has to be
//! kept around. This information is carried in the bundle.

use std::rc::Rc;

use lynx_sys::Element;

use crate::html::AnyScope;
use crate::virtual_dom::VNode;

mod bcomp;
mod blist;
mod bnode;
mod bportal;
mod btag;
mod btext;
mod position;
mod subtree_root;

mod traits;
mod utils;

use bcomp::BComp;
use blist::BList;
use bnode::BNode;
use bportal::BPortal;
use btag::{BTag, Registry};
use btext::BText;
pub(crate) use position::{DomSlot, DynamicDomSlot};
pub use subtree_root::BSubtree;
use subtree_root::EventDescriptor;
use traits::{Reconcilable, ReconcileTarget};
use utils::test_log;

/// A Bundle.
///
/// Each component holds a bundle that represents a realised layout, designated by a [VNode].
///
/// This is not to be confused with [BComp], which represents a component in the position of a
/// bundle layout.
#[derive(Debug)]
pub(crate) struct Bundle(BNode);

impl Bundle {
    /// Creates a new bundle.
    pub const fn new() -> Self {
        Self(BNode::List(BList::new()))
    }

    /// Shifts the bundle into a different position.
    pub fn shift(&self, next_parent: &Rc<Element>, slot: DomSlot) {
        self.0.shift(next_parent, slot);
    }

    /// Applies a virtual dom layout to current bundle.
    pub fn reconcile(
        &mut self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
        next_node: VNode,
    ) -> DomSlot {
        next_node.reconcile_node(root, parent_scope, parent, slot, &mut self.0)
    }

    /// Detaches current bundle.
    pub fn detach(self, root: &BSubtree, parent: &Rc<Element>, parent_to_detach: bool) {
        self.0.detach(root, parent, parent_to_detach);
    }
}
