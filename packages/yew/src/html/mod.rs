//! The main html module which defines components, listeners, and class helpers.

mod classes;
mod component;
mod conversion;
mod error;
mod listener;

use std::cell::RefCell;
use std::rc::Rc;

pub use classes::*;
pub use component::*;
pub use conversion::*;
pub use error::*;
pub use listener::*;
use rust_wasm_binding::Element;

use crate::sealed::Sealed;
use crate::virtual_dom::VNode;
#[cfg(feature = "csr")]
use crate::virtual_dom::VPortal;

/// A type which expected as a result of `view` function implementation.
pub type Html = VNode;

/// An enhanced type of `Html` returned in suspendible function components.
pub type HtmlResult = RenderResult<Html>;

impl Sealed for HtmlResult {}
impl Sealed for Html {}

/// A trait to translate into a [`HtmlResult`].
pub trait IntoHtmlResult: Sealed {
    /// Performs the conversion.
    fn into_html_result(self) -> HtmlResult;
}

impl IntoHtmlResult for HtmlResult {
    #[inline(always)]
    fn into_html_result(self) -> HtmlResult {
        self
    }
}
impl IntoHtmlResult for Html {
    #[inline(always)]
    fn into_html_result(self) -> HtmlResult {
        Ok(self)
    }
}

/// Wrapped Node reference for later use in Component lifecycle methods.
///
/// Stores an `Rc<Element>` handle to the underlying DOM node, giving user
/// code type-safe access to the element and keeping it alive via shared
/// ownership. Query element state through the [`rust_wasm_binding::ElementOps`]
/// / [`rust_wasm_binding::NodeOps`] traits on the returned handle.
#[derive(Default, Clone, ImplicitClone)]
pub struct NodeRef(Rc<RefCell<NodeRefInner>>);

impl PartialEq for NodeRef {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0.as_ptr(), other.0.as_ptr())
    }
}

impl std::fmt::Debug for NodeRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use rust_wasm_binding::NodeOps;
        write!(
            f,
            "NodeRef {{ references: {:?} }}",
            self.get().map(|e| e.id())
        )
    }
}

#[derive(Debug, Default, Clone)]
struct NodeRefInner {
    node: Option<Rc<Element>>,
}

impl NodeRef {
    /// Get a shared handle to the wrapped element, if one has been attached.
    pub fn get(&self) -> Option<Rc<Element>> {
        self.0.borrow().node.clone()
    }
}

#[cfg(feature = "csr")]
mod feat_csr {
    use super::*;

    impl NodeRef {
        pub(crate) fn set(&self, new_ref: Option<Rc<Element>>) {
            self.0.borrow_mut().node = new_ref;
        }
    }
}

/// Render children into a DOM node that exists outside the hierarchy of the parent
/// component.
#[cfg(feature = "csr")]
pub fn create_portal(child: Html, host: Rc<Element>) -> Html {
    VNode::VPortal(Rc::new(VPortal::new(child, host)))
}
