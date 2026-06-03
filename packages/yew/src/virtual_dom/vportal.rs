//! This module contains the implementation of a portal `VPortal`.

use std::rc::Rc;

use rust_wasm_binding::{Element, ExternRef};

use super::VNode;

/// Portal target. Wrapped in [`Rc`] so the user code can keep its own clone
/// of the host element alive while yew renders into it.
pub type PortalHost = Rc<Element>;

#[derive(Debug, Clone)]
pub struct VPortal {
    /// The element under which the content is inserted.
    pub host: PortalHost,
    /// The next sibling after the inserted content. Must be a
    /// child of `host` if set.
    pub inner_sibling: Option<ExternRef>,
    /// The inserted node
    pub node: VNode,
}

impl PartialEq for VPortal {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.host, &other.host)
            && self.inner_sibling == other.inner_sibling
            && self.node == other.node
    }
}

impl VPortal {
    /// Creates a [VPortal] rendering `content` in the DOM hierarchy under `host`.
    pub fn new(content: VNode, host: PortalHost) -> Self {
        Self {
            host,
            inner_sibling: None,
            node: content,
        }
    }

    /// Creates a [VPortal] rendering `content` in the DOM hierarchy under `host`.
    /// If `inner_sibling` is given, the content is inserted before that node.
    /// The parent of `inner_sibling`, if given, must be `host`.
    pub fn new_before(content: VNode, host: PortalHost, inner_sibling: Option<ExternRef>) -> Self {
        Self {
            host,
            inner_sibling,
            node: content,
        }
    }
}
