//! This module contains fragments implementation.
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use super::{Key, VNode};

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FullyKeyedState {
    KnownFullyKeyed,
    KnownMissingKeys,
    Unknown,
}

/// This struct represents a fragment of the Virtual DOM tree.
#[derive(Clone, Debug)]
pub struct VList {
    /// The list of child [VNode]s
    pub(crate) children: Option<Rc<Vec<VNode>>>,

    /// All [VNode]s in the VList have keys
    fully_keyed: FullyKeyedState,

    pub key: Option<Key>,
}

impl PartialEq for VList {
    fn eq(&self, other: &Self) -> bool {
        if self.key != other.key {
            return false;
        }

        match (self.children.as_ref(), other.children.as_ref()) {
            (Some(a), Some(b)) => a == b,
            (Some(a), None) => a.is_empty(),
            (None, Some(b)) => b.is_empty(),
            (None, None) => true,
        }
    }
}

impl Default for VList {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for VList {
    type Target = Vec<VNode>;

    fn deref(&self) -> &Self::Target {
        match self.children {
            Some(ref m) => m,
            None => {
                // This can be replaced with `const { &Vec::new() }` in Rust 1.79.
                const EMPTY: &Vec<VNode> = &Vec::new();
                EMPTY
            }
        }
    }
}

impl DerefMut for VList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.fully_keyed = FullyKeyedState::Unknown;
        self.children_mut()
    }
}

impl<A: Into<VNode>> FromIterator<A> for VList {
    fn from_iter<T: IntoIterator<Item = A>>(iter: T) -> Self {
        let children = iter.into_iter().map(|n| n.into()).collect::<Vec<_>>();
        if children.is_empty() {
            VList::new()
        } else {
            VList {
                children: Some(Rc::new(children)),
                fully_keyed: FullyKeyedState::Unknown,
                key: None,
            }
        }
    }
}

impl From<Option<Rc<Vec<VNode>>>> for VList {
    fn from(children: Option<Rc<Vec<VNode>>>) -> Self {
        if children.as_ref().map(|x| x.is_empty()).unwrap_or(true) {
            VList::new()
        } else {
            let mut vlist = VList {
                children,
                fully_keyed: FullyKeyedState::Unknown,
                key: None,
            };
            vlist.recheck_fully_keyed();
            vlist
        }
    }
}

impl From<Vec<VNode>> for VList {
    fn from(children: Vec<VNode>) -> Self {
        if children.is_empty() {
            VList::new()
        } else {
            let mut vlist = VList {
                children: Some(Rc::new(children)),
                fully_keyed: FullyKeyedState::Unknown,
                key: None,
            };
            vlist.recheck_fully_keyed();
            vlist
        }
    }
}

impl From<VNode> for VList {
    fn from(child: VNode) -> Self {
        let mut vlist = VList {
            children: Some(Rc::new(vec![child])),
            fully_keyed: FullyKeyedState::Unknown,
            key: None,
        };
        vlist.recheck_fully_keyed();
        vlist
    }
}

impl VList {
    /// Creates a new empty [VList] instance.
    pub const fn new() -> Self {
        Self {
            children: None,
            key: None,
            fully_keyed: FullyKeyedState::KnownFullyKeyed,
        }
    }

    /// Creates a new [VList] instance with children.
    pub fn with_children(children: Vec<VNode>, key: Option<Key>) -> Self {
        let mut vlist = VList::from(children);
        vlist.key = key;
        vlist
    }

    #[doc(hidden)]
    /// Used by `html!` to avoid calling `.recheck_fully_keyed()` when possible.
    pub fn __macro_new(
        children: Vec<VNode>,
        key: Option<Key>,
        fully_keyed: FullyKeyedState,
    ) -> Self {
        VList {
            children: Some(Rc::new(children)),
            fully_keyed,
            key,
        }
    }

    // Returns a mutable reference to children, allocates the children if it hasn't been done.
    //
    // This method does not reassign key state. So it should only be used internally.
    fn children_mut(&mut self) -> &mut Vec<VNode> {
        loop {
            match self.children {
                Some(ref mut m) => return Rc::make_mut(m),
                None => {
                    self.children = Some(Rc::new(Vec::new()));
                }
            }
        }
    }

    /// Add [VNode] child.
    pub fn add_child(&mut self, child: VNode) {
        if self.fully_keyed == FullyKeyedState::KnownFullyKeyed && !child.has_key() {
            self.fully_keyed = FullyKeyedState::KnownMissingKeys;
        }
        self.children_mut().push(child);
    }

    /// Add multiple [VNode] children.
    pub fn add_children(&mut self, children: impl IntoIterator<Item = VNode>) {
        let it = children.into_iter();
        let bound = it.size_hint();
        self.children_mut().reserve(bound.1.unwrap_or(bound.0));
        for ch in it {
            self.add_child(ch);
        }
    }

    /// Recheck, if the all the children have keys.
    ///
    /// You can run this, after modifying the child list through the [DerefMut] implementation of
    /// [VList], to precompute an internally kept flag, which speeds up reconciliation later.
    pub fn recheck_fully_keyed(&mut self) {
        self.fully_keyed = if self.fully_keyed() {
            FullyKeyedState::KnownFullyKeyed
        } else {
            FullyKeyedState::KnownMissingKeys
        };
    }

    pub(crate) fn fully_keyed(&self) -> bool {
        match self.fully_keyed {
            FullyKeyedState::KnownFullyKeyed => true,
            FullyKeyedState::KnownMissingKeys => false,
            FullyKeyedState::Unknown => self.iter().all(|c| c.has_key()),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::virtual_dom::{VTag, VText};

    #[test]
    fn mutably_change_children() {
        let mut vlist = VList::new();
        assert_eq!(
            vlist.fully_keyed,
            FullyKeyedState::KnownFullyKeyed,
            "should start fully keyed"
        );
        // add a child that is keyed
        vlist.add_child(VNode::VTag({
            let mut tag = VTag::new("a");
            tag.key = Some(42u32.into());
            tag.into()
        }));
        assert_eq!(
            vlist.fully_keyed,
            FullyKeyedState::KnownFullyKeyed,
            "should still be fully keyed"
        );
        assert_eq!(vlist.len(), 1, "should contain 1 child");
        // now add a child that is not keyed
        vlist.add_child(VNode::VText(VText::new("lorem ipsum")));
        assert_eq!(
            vlist.fully_keyed,
            FullyKeyedState::KnownMissingKeys,
            "should not be fully keyed, text tags have no key"
        );
        let _: &mut [VNode] = &mut vlist; // Use deref mut
        assert_eq!(
            vlist.fully_keyed,
            FullyKeyedState::Unknown,
            "key state should be unknown, since it was potentially modified through children"
        );
    }
}
