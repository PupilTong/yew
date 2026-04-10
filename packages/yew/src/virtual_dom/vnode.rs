//! This module contains the implementation of abstract virtual node.

use std::cmp::PartialEq;
use std::iter::FromIterator;
use std::rc::Rc;
use std::{fmt, mem};

use rust_wasm_binding::{Element, NodeOps};

use super::{Key, VChild, VComp, VList, VPortal, VSuspense, VTag, VText};
use crate::html::{BaseComponent, ImplicitClone};

/// Bind virtual element to a DOM reference.
#[derive(Clone, ImplicitClone)]
#[must_use = "html does not do anything unless returned to Yew for rendering."]
pub enum VNode {
    /// A bind between `VTag` and an element node.
    VTag(Rc<VTag>),
    /// A bind between `VText` and a text node.
    VText(VText),
    /// A bind between `VComp` and an element node.
    VComp(Rc<VComp>),
    /// A holder for a list of other nodes.
    VList(Rc<VList>),
    /// A portal to another part of the document
    VPortal(Rc<VPortal>),
    /// A holder for a raw, user-supplied DOM element node. The wrapper is
    /// shared via `Rc` so cloning a [`VNode`] does not duplicate the
    /// underlying slab id.
    VRef(Rc<Element>),
    /// A suspendible document fragment.
    VSuspense(Rc<VSuspense>),
}

impl PartialEq for VNode {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (VNode::VTag(left), VNode::VTag(right)) => left == right,
            (VNode::VText(left), VNode::VText(right)) => left == right,
            (VNode::VComp(left), VNode::VComp(right)) => left == right,
            (VNode::VList(left), VNode::VList(right)) => left == right,
            (VNode::VPortal(left), VNode::VPortal(right)) => left == right,
            // Two `VRef`s are equal when they refer to the same Paws slab id —
            // either through pointer equality of the `Rc` (cheap) or by id
            // comparison as a fallback. `Element` itself does not implement
            // `PartialEq` because two distinct `Element` values cannot exist
            // for the same slab id (Drop would double-free), but two `Rc`
            // clones of the same `Element` legitimately compare equal here.
            (VNode::VRef(left), VNode::VRef(right)) => {
                Rc::ptr_eq(left, right) || left.id() == right.id()
            }
            (VNode::VSuspense(left), VNode::VSuspense(right)) => left == right,
            _ => false,
        }
    }
}

impl VNode {
    pub fn key(&self) -> Option<&Key> {
        match self {
            VNode::VComp(vcomp) => vcomp.key.as_ref(),
            VNode::VList(vlist) => vlist.key.as_ref(),
            VNode::VRef(_) => None,
            VNode::VTag(vtag) => vtag.key.as_ref(),
            VNode::VText(_) => None,
            VNode::VPortal(vportal) => vportal.node.key(),
            VNode::VSuspense(vsuspense) => vsuspense.key.as_ref(),
        }
    }

    /// Returns true if the [VNode] has a key.
    pub fn has_key(&self) -> bool {
        self.key().is_some()
    }

    /// Acquires a mutable reference of current VNode as a VList.
    ///
    /// Creates a VList with the current node as the first child if current VNode is not a VList.
    pub fn to_vlist_mut(&mut self) -> &mut VList {
        loop {
            match *self {
                Self::VList(ref mut m) => return Rc::make_mut(m),
                _ => {
                    *self = VNode::VList(Rc::new(VList::from(mem::take(self))));
                }
            }
        }
    }
}

impl Default for VNode {
    fn default() -> Self {
        VNode::VList(Rc::new(VList::default()))
    }
}

impl From<VText> for VNode {
    #[inline]
    fn from(vtext: VText) -> Self {
        VNode::VText(vtext)
    }
}

impl From<VList> for VNode {
    #[inline]
    fn from(vlist: VList) -> Self {
        VNode::VList(Rc::new(vlist))
    }
}

impl From<VTag> for VNode {
    #[inline]
    fn from(vtag: VTag) -> Self {
        VNode::VTag(Rc::new(vtag))
    }
}

impl From<VComp> for VNode {
    #[inline]
    fn from(vcomp: VComp) -> Self {
        VNode::VComp(Rc::new(vcomp))
    }
}

impl From<VSuspense> for VNode {
    #[inline]
    fn from(vsuspense: VSuspense) -> Self {
        VNode::VSuspense(Rc::new(vsuspense))
    }
}

impl From<VPortal> for VNode {
    #[inline]
    fn from(vportal: VPortal) -> Self {
        VNode::VPortal(Rc::new(vportal))
    }
}

impl<COMP> From<VChild<COMP>> for VNode
where
    COMP: BaseComponent,
{
    fn from(vchild: VChild<COMP>) -> Self {
        VNode::VComp(Rc::new(VComp::from(vchild)))
    }
}

impl<T: ToString> From<T> for VNode {
    fn from(value: T) -> Self {
        VNode::VText(VText::new(value.to_string()))
    }
}

impl<A: Into<VNode>> FromIterator<A> for VNode {
    fn from_iter<T: IntoIterator<Item = A>>(iter: T) -> Self {
        VNode::VList(Rc::new(VList::from_iter(
            iter.into_iter().map(|n| n.into()),
        )))
    }
}

impl fmt::Debug for VNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            VNode::VTag(ref vtag) => vtag.fmt(f),
            VNode::VText(ref vtext) => vtext.fmt(f),
            VNode::VComp(ref vcomp) => vcomp.fmt(f),
            VNode::VList(ref vlist) => vlist.fmt(f),
            VNode::VRef(ref vref) => write!(f, "VRef({})", vref.id()),
            VNode::VPortal(ref vportal) => vportal.fmt(f),
            VNode::VSuspense(ref vsuspense) => vsuspense.fmt(f),
        }
    }
}
