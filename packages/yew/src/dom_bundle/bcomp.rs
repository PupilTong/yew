//! This module contains the bundle implementation of a virtual component [BComp].

use std::any::TypeId;
use std::borrow::Borrow;
use std::fmt;
use std::rc::Rc;

use lynx_sys::Element;

use super::{BNode, BSubtree, DomSlot, DynamicDomSlot, Reconcilable, ReconcileTarget};
use crate::html::{AnyScope, Scoped};
use crate::virtual_dom::{Key, VComp};

/// A virtual component. Compare with [VComp].
pub(super) struct BComp {
    type_id: TypeId,
    scope: Box<dyn Scoped>,
    /// An internal [`DomSlot`] passed around to track this components position. This
    /// will dynamically adjust when a lifecycle changes the render state of this component.
    own_position: DynamicDomSlot,
    key: Option<Key>,
}

impl BComp {
    /// Get the key of the underlying component
    pub fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }
}

impl fmt::Debug for BComp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BComp")
            .field("root", &self.scope.as_ref().render_state())
            .finish()
    }
}

impl ReconcileTarget for BComp {
    fn detach(self, _root: &BSubtree, _parent: &Rc<Element>, parent_to_detach: bool) {
        self.scope.destroy_boxed(parent_to_detach);
    }

    fn shift(&self, next_parent: &Rc<Element>, slot: DomSlot) -> DomSlot {
        self.scope.shift_node(next_parent, slot);

        self.own_position.to_position()
    }
}

impl Reconcilable for VComp {
    type Bundle = BComp;

    fn attach(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
    ) -> (DomSlot, Self::Bundle) {
        let VComp {
            type_id,
            mountable,
            key,
            ..
        } = self;

        let (scope, internal_ref) = mountable.mount(root, parent_scope, parent, slot);

        (
            internal_ref.to_position(),
            BComp {
                type_id,
                scope,
                own_position: internal_ref,
                key,
            },
        )
    }

    fn reconcile_node(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
        bundle: &mut BNode,
    ) -> DomSlot {
        match bundle {
            // If the existing bundle is the same type, reuse it and update its properties
            BNode::Comp(ref mut bcomp)
                if self.type_id == bcomp.type_id && self.key == bcomp.key =>
            {
                self.reconcile(root, parent_scope, parent, slot, bcomp)
            }
            _ => self.replace(root, parent_scope, parent, slot, bundle),
        }
    }

    fn reconcile(
        self,
        _root: &BSubtree,
        _parent_scope: &AnyScope,
        _parent: &Rc<Element>,
        slot: DomSlot,
        bcomp: &mut Self::Bundle,
    ) -> DomSlot {
        let VComp { mountable, key, .. } = self;

        bcomp.key = key;
        mountable.reuse(bcomp.scope.borrow(), slot);
        bcomp.own_position.to_position()
    }
}

#[cfg(test)]
mod tests {
    use crate::html::{Component, Context, Html};
    use crate::virtual_dom::{Key, VChild, VNode};
    use crate::{html, Properties};

    struct Comp;

    #[derive(Clone, PartialEq, Properties)]
    struct Props {
        #[prop_or_default]
        field_1: u32,
        #[prop_or_default]
        field_2: u32,
    }

    impl Component for Comp {
        type Message = ();
        type Properties = Props;

        fn create(_: &Context<Self>) -> Self {
            Comp
        }

        fn update(&mut self, _ctx: &Context<Self>, _: Self::Message) -> bool {
            unimplemented!()
        }

        fn view(&self, _ctx: &Context<Self>) -> Html {
            html! { <div/> }
        }
    }

    #[test]
    fn set_properties_to_component() {
        html! { <Comp /> };
        html! { <Comp field_1=1 /> };
        html! { <Comp field_2=2 /> };
        html! { <Comp field_1=1 field_2=2 /> };
        let props = Props {
            field_1: 1,
            field_2: 1,
        };
        html! { <Comp ..props /> };
    }

    #[test]
    fn set_component_key() {
        let test_key: Key = "test".to_string().into();
        let check_key = |vnode: VNode| {
            assert_eq!(vnode.key(), Some(&test_key));
        };
        let props = Props {
            field_1: 1,
            field_2: 1,
        };
        let props_2 = props.clone();
        check_key(html! { <Comp key={test_key.clone()} /> });
        check_key(html! { <Comp key={test_key.clone()} field_1=1 /> });
        check_key(html! { <Comp field_1=1 key={test_key.clone()} /> });
        check_key(html! { <Comp key={test_key.clone()} ..props /> });
        check_key(html! { <Comp key={test_key.clone()} ..props_2 /> });
    }

    #[test]
    fn vchild_partialeq() {
        let vchild1: VChild<Comp> = VChild::new(
            Props {
                field_1: 1,
                field_2: 1,
            },
            None,
        );
        let vchild2: VChild<Comp> = VChild::new(
            Props {
                field_1: 1,
                field_2: 1,
            },
            None,
        );
        let vchild3: VChild<Comp> = VChild::new(
            Props {
                field_1: 2,
                field_2: 2,
            },
            None,
        );
        assert_eq!(vchild1, vchild2);
        assert_ne!(vchild1, vchild3);
        assert_ne!(vchild2, vchild3);
    }
}
