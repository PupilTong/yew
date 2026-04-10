use std::rc::Rc;

use rust_wasm_binding::Element;

use super::{BNode, BSubtree, DomSlot};
use crate::html::AnyScope;

/// A Reconcile Target.
///
/// When a [Reconcilable] is attached, a reconcile target is created to store additional
/// information.
pub(super) trait ReconcileTarget {
    /// Remove self from parent.
    ///
    /// Parent to detach is `true` if the parent element will also be detached.
    fn detach(self, root: &BSubtree, parent: &Rc<Element>, parent_to_detach: bool);

    /// Move elements from one parent to another parent.
    /// This is for example used by `VSuspense` to preserve component state without detaching
    /// (which destroys component state).
    fn shift(&self, next_parent: &Rc<Element>, slot: DomSlot) -> DomSlot;
}

/// This trait provides features to update a tree by calculating a difference against another tree.
pub(super) trait Reconcilable {
    type Bundle: ReconcileTarget;

    /// Attach a virtual node to the DOM tree.
    ///
    /// Parameters:
    /// - `root`: bundle of the subtree root
    /// - `parent_scope`: the parent `Scope` used for passing messages to the parent `Component`.
    /// - `parent`: shared handle to the parent element in the DOM. Trait methods take
    ///   `&Rc<Element>` (not `&Element`) so an implementation that needs to keep a long-lived
    ///   reference to the parent — most notably `Scope::mount_in_place` storing the parent inside
    ///   `ComponentRenderState` — can `Rc::clone` it.
    /// - `slot`: to find where to put the node.
    ///
    /// Returns a reference to the newly inserted element.
    /// The [`DomSlot`] points the first element (if there are multiple nodes created),
    /// or is the passed in `slot` if there are no element is created.
    fn attach(
        self,

        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
    ) -> (DomSlot, Self::Bundle);

    /// Scoped diff apply to other tree.
    ///
    /// Virtual rendering for the node. It uses parent node and existing
    /// children (virtual and DOM) to check the difference and apply patches to
    /// the actual DOM representation.
    ///
    /// Parameters:
    /// - `parent_scope`: the parent `Scope` used for passing messages to the parent `Component`.
    /// - `parent`: shared handle to the parent element in the DOM.
    /// - `slot`: the slot in `parent`'s children where to put the node.
    /// - `bundle`: the node that this node will be replacing in the DOM. This method will remove
    ///   the `bundle` from the `parent` if it is of the wrong kind, and otherwise reuse it.
    ///
    /// Returns a reference to the newly inserted element.
    fn reconcile_node(
        self,

        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
        bundle: &mut BNode,
    ) -> DomSlot;

    fn reconcile(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
        bundle: &mut Self::Bundle,
    ) -> DomSlot;

    /// Replace an existing bundle by attaching self and detaching the existing one
    fn replace(
        self,

        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
        bundle: &mut BNode,
    ) -> DomSlot
    where
        Self: Sized,
        Self::Bundle: Into<BNode>,
    {
        let (self_ref, self_) = self.attach(root, parent_scope, parent, slot);
        let ancestor = std::mem::replace(bundle, self_.into());
        ancestor.detach(root, parent, false);
        self_ref
    }
}
