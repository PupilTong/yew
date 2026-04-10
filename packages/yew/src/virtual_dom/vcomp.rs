//! This module contains the implementation of a virtual component (`VComp`).

use std::any::{Any, TypeId};
use std::fmt;
use std::rc::Rc;

#[cfg(feature = "csr")]
use rust_wasm_binding::Element;

use super::Key;
#[cfg(feature = "csr")]
use crate::dom_bundle::{BSubtree, DomSlot, DynamicDomSlot};
use crate::html::BaseComponent;
#[cfg(feature = "csr")]
use crate::html::Scoped;
#[cfg(feature = "csr")]
use crate::html::{AnyScope, Scope};

/// A virtual component.
pub struct VComp {
    pub(crate) type_id: TypeId,
    pub(crate) mountable: Box<dyn Mountable>,
    pub(crate) key: Option<Key>,
    // for some reason, this reduces the bundle size by ~2-3 KBs
    _marker: u32,
}

impl fmt::Debug for VComp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VComp")
            .field("type_id", &self.type_id)
            .field("mountable", &"..")
            .field("key", &self.key)
            .finish()
    }
}

impl Clone for VComp {
    fn clone(&self) -> Self {
        Self {
            type_id: self.type_id,
            mountable: self.mountable.copy(),
            key: self.key.clone(),
            _marker: 0,
        }
    }
}

pub(crate) trait Mountable {
    fn copy(&self) -> Box<dyn Mountable>;

    fn mountable_eq(&self, rhs: &dyn Mountable) -> bool;
    fn as_any(&self) -> &dyn Any;

    #[cfg(feature = "csr")]
    fn mount(
        self: Box<Self>,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
    ) -> (Box<dyn Scoped>, DynamicDomSlot);

    #[cfg(feature = "csr")]
    fn reuse(self: Box<Self>, scope: &dyn Scoped, slot: DomSlot);
}

pub(crate) struct PropsWrapper<COMP: BaseComponent> {
    props: Rc<COMP::Properties>,
}

impl<COMP: BaseComponent> PropsWrapper<COMP> {
    pub fn new(props: Rc<COMP::Properties>) -> Self {
        Self { props }
    }
}

impl<COMP: BaseComponent> Mountable for PropsWrapper<COMP> {
    fn copy(&self) -> Box<dyn Mountable> {
        let wrapper: PropsWrapper<COMP> = PropsWrapper {
            props: Rc::clone(&self.props),
        };
        Box::new(wrapper)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn mountable_eq(&self, rhs: &dyn Mountable) -> bool {
        rhs.as_any()
            .downcast_ref::<Self>()
            .map(|rhs| self.props == rhs.props)
            .unwrap_or(false)
    }

    #[cfg(feature = "csr")]
    fn mount(
        self: Box<Self>,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
    ) -> (Box<dyn Scoped>, DynamicDomSlot) {
        let scope: Scope<COMP> = Scope::new(Some(parent_scope.clone()));
        // `mount_in_place` takes ownership of the parent `Rc<Element>` so
        // it can store a clone in `ComponentRenderState::Render`. We clone
        // here from the borrowed Rc handed to us through the trait.
        let own_slot = scope.mount_in_place(root.clone(), Rc::clone(parent), slot, self.props);

        (Box::new(scope), own_slot)
    }

    #[cfg(feature = "csr")]
    fn reuse(self: Box<Self>, scope: &dyn Scoped, slot: DomSlot) {
        let scope: Scope<COMP> = scope.to_any().downcast::<COMP>();
        scope.reuse(self.props, slot);
    }
}

/// A virtual child component.
pub struct VChild<COMP: BaseComponent> {
    /// The component properties
    pub props: Rc<COMP::Properties>,
    /// Reference to the mounted node
    key: Option<Key>,
}

impl<COMP: BaseComponent> implicit_clone::ImplicitClone for VChild<COMP> {}

impl<COMP: BaseComponent> Clone for VChild<COMP> {
    fn clone(&self) -> Self {
        VChild {
            props: Rc::clone(&self.props),
            key: self.key.clone(),
        }
    }
}

impl<COMP: BaseComponent> PartialEq for VChild<COMP>
where
    COMP::Properties: PartialEq,
{
    fn eq(&self, other: &VChild<COMP>) -> bool {
        self.props == other.props
    }
}

impl<COMP> VChild<COMP>
where
    COMP: BaseComponent,
{
    /// Creates a child component that can be accessed and modified by its parent.
    pub fn new(props: COMP::Properties, key: Option<Key>) -> Self {
        Self {
            props: Rc::new(props),
            key,
        }
    }
}

impl<COMP> VChild<COMP>
where
    COMP: BaseComponent,
    COMP::Properties: Clone,
{
    /// Get a mutable reference to the underlying properties.
    pub fn get_mut(&mut self) -> &mut COMP::Properties {
        Rc::make_mut(&mut self.props)
    }
}

impl<COMP> From<VChild<COMP>> for VComp
where
    COMP: BaseComponent,
{
    fn from(vchild: VChild<COMP>) -> Self {
        VComp::new::<COMP>(vchild.props, vchild.key)
    }
}

impl VComp {
    /// Creates a new `VComp` instance.
    pub fn new<COMP>(props: Rc<COMP::Properties>, key: Option<Key>) -> Self
    where
        COMP: BaseComponent,
    {
        VComp {
            type_id: TypeId::of::<COMP>(),
            mountable: Box::new(PropsWrapper::<COMP>::new(props)),
            key,
            _marker: 0,
        }
    }
}

impl PartialEq for VComp {
    fn eq(&self, other: &VComp) -> bool {
        self.key == other.key
            && self.type_id == other.type_id
            && self.mountable.mountable_eq(other.mountable.as_ref())
    }
}

impl<COMP: BaseComponent> fmt::Debug for VChild<COMP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VChild<_>")
    }
}
