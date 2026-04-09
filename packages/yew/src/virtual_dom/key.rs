//! This module contains the implementation yew's virtual nodes' keys.

use std::fmt::{self, Display, Formatter};
use std::ops::Deref;
use std::rc::Rc;

use crate::html::ImplicitClone;

/// Represents the (optional) key of Yew's virtual nodes.
///
/// Keys are cheap to clone.
#[derive(Clone, ImplicitClone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct Key {
    key: Rc<str>,
}

impl Display for Key {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.key.fmt(f)
    }
}

impl Deref for Key {
    type Target = str;

    fn deref(&self) -> &str {
        self.key.as_ref()
    }
}

impl From<Rc<str>> for Key {
    fn from(key: Rc<str>) -> Self {
        Self { key }
    }
}

impl From<&'_ str> for Key {
    fn from(key: &'_ str) -> Self {
        let key: Rc<str> = Rc::from(key);
        Self::from(key)
    }
}

impl From<String> for Key {
    fn from(key: String) -> Self {
        Self::from(key.as_str())
    }
}

macro_rules! key_impl_from_to_string {
    ($type:ty) => {
        impl From<$type> for Key {
            fn from(key: $type) -> Self {
                Self::from(key.to_string().as_str())
            }
        }
    };
}

key_impl_from_to_string!(char);
key_impl_from_to_string!(u8);
key_impl_from_to_string!(u16);
key_impl_from_to_string!(u32);
key_impl_from_to_string!(u64);
key_impl_from_to_string!(u128);
key_impl_from_to_string!(usize);
key_impl_from_to_string!(i8);
key_impl_from_to_string!(i16);
key_impl_from_to_string!(i32);
key_impl_from_to_string!(i64);
key_impl_from_to_string!(i128);
key_impl_from_to_string!(isize);
