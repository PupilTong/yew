use super::{Key, VNode};
use crate::html::ImplicitClone;

/// This struct represents a suspendable DOM fragment.
#[derive(Clone, ImplicitClone, Debug, PartialEq)]
pub struct VSuspense {
    /// Child nodes.
    pub(crate) children: VNode,
    /// Fallback nodes when suspended.
    pub(crate) fallback: VNode,
    /// Whether the current status is suspended.
    pub(crate) suspended: bool,
    /// The Key.
    pub(crate) key: Option<Key>,
}

impl VSuspense {
    pub fn new(children: VNode, fallback: VNode, suspended: bool, key: Option<Key>) -> Self {
        Self {
            children,
            fallback,
            suspended,
            key,
        }
    }
}
