//! This module contains the implementation of a virtual text node `VText`.

use std::cmp::PartialEq;

use super::AttrValue;
use crate::html::ImplicitClone;

/// A type for a virtual
/// [`TextNode`](https://developer.mozilla.org/en-US/docs/Web/API/Document/createTextNode)
/// representation.
#[derive(Clone, ImplicitClone)]
pub struct VText {
    /// Contains a text of the node.
    pub text: AttrValue,
}

impl VText {
    /// Creates new virtual text node with a content.
    pub fn new(text: impl Into<AttrValue>) -> Self {
        VText { text: text.into() }
    }
}

impl std::fmt::Debug for VText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VText {{ text: \"{}\" }}", self.text)
    }
}

impl PartialEq for VText {
    fn eq(&self, other: &VText) -> bool {
        self.text == other.text
    }
}

impl<T: ToString> From<T> for VText {
    fn from(value: T) -> Self {
        VText::new(value.to_string())
    }
}
