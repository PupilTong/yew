//! This module contains the implementation of a portal `VPortal`.

use super::VNode;

#[derive(Debug, Clone, PartialEq)]
pub struct VPortal {
    /// Paws node id of the element under which the content is inserted.
    pub host: i32,
    /// The next sibling after the inserted content (Paws node id). Must be a
    /// child of `host` if set.
    pub inner_sibling: Option<i32>,
    /// The inserted node
    pub node: VNode,
}

impl VPortal {
    /// Creates a [VPortal] rendering `content` in the DOM hierarchy under `host`.
    pub fn new(content: VNode, host: i32) -> Self {
        Self {
            host,
            inner_sibling: None,
            node: content,
        }
    }

    /// Creates a [VPortal] rendering `content` in the DOM hierarchy under `host`.
    /// If `inner_sibling` is given, the content is inserted before that node.
    /// The parent of `inner_sibling`, if given, must be `host`.
    pub fn new_before(content: VNode, host: i32, inner_sibling: Option<i32>) -> Self {
        Self {
            host,
            inner_sibling,
            node: content,
        }
    }
}
