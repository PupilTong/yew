//! This module contains the bundle version of a suspense [BSuspense]

use gloo::utils::document;
use web_sys::Element;

use super::{BNode, BSubtree, DomSlot, Reconcilable, ReconcileTarget};
use crate::html::AnyScope;
use crate::virtual_dom::{Key, VSuspense};

#[derive(Debug)]
enum Fallback {
    /// Suspense Fallback with fallback being rendered as placeholder.
    Bundle(BNode),
}

/// The bundle implementation to [VSuspense]
#[derive(Debug)]
pub(super) struct BSuspense {
    children_bundle: BNode,
    /// The suspense is suspended if fallback contains [Some] bundle
    fallback: Option<Fallback>,
    detached_parent: Element,
    key: Option<Key>,
}

impl BSuspense {
    /// Get the key of the underlying suspense
    pub fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }
}

impl ReconcileTarget for BSuspense {
    fn detach(self, root: &BSubtree, parent: &Element, parent_to_detach: bool) {
        match self.fallback {
            Some(m) => {
                let Fallback::Bundle(bundle) = m;
                bundle.detach(root, parent, parent_to_detach);

                self.children_bundle
                    .detach(root, &self.detached_parent, false);
            }
            None => {
                self.children_bundle.detach(root, parent, parent_to_detach);
            }
        }
    }

    fn shift(&self, next_parent: &Element, slot: DomSlot) -> DomSlot {
        match self.fallback.as_ref() {
            Some(Fallback::Bundle(bundle)) => bundle.shift(next_parent, slot),
            None => self.children_bundle.shift(next_parent, slot),
        }
    }
}

impl Reconcilable for VSuspense {
    type Bundle = BSuspense;

    fn attach(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Element,
        slot: DomSlot,
    ) -> (DomSlot, Self::Bundle) {
        let VSuspense {
            children,
            fallback,
            suspended,
            key,
        } = self;
        let detached_parent = document()
            .create_element("div")
            .expect("failed to create detached element");

        // When it's suspended, we render children into an element that is detached from the dom
        // tree while rendering fallback UI into the original place where children resides in.
        if suspended {
            let (_child_ref, children_bundle) =
                children.attach(root, parent_scope, &detached_parent, DomSlot::at_end());
            let (fallback_ref, fallback) = fallback.attach(root, parent_scope, parent, slot);
            (
                fallback_ref,
                BSuspense {
                    children_bundle,
                    fallback: Some(Fallback::Bundle(fallback)),
                    detached_parent,
                    key,
                },
            )
        } else {
            let (child_ref, children_bundle) = children.attach(root, parent_scope, parent, slot);
            (
                child_ref,
                BSuspense {
                    children_bundle,
                    fallback: None,
                    detached_parent,
                    key,
                },
            )
        }
    }

    fn reconcile_node(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Element,
        slot: DomSlot,
        bundle: &mut BNode,
    ) -> DomSlot {
        match bundle {
            // We only preserve the child state if they are the same suspense.
            BNode::Suspense(m) if m.key == self.key => {
                self.reconcile(root, parent_scope, parent, slot, m)
            }
            _ => self.replace(root, parent_scope, parent, slot, bundle),
        }
    }

    fn reconcile(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Element,
        slot: DomSlot,
        suspense: &mut Self::Bundle,
    ) -> DomSlot {
        let VSuspense {
            children,
            fallback: vfallback,
            suspended,
            key: _,
        } = self;

        let children_bundle = &mut suspense.children_bundle;
        // no need to update key & detached_parent

        // When it's suspended, we render children into an element that is detached from the dom
        // tree while rendering fallback UI into the original place where children resides in.
        match (suspended, &mut suspense.fallback) {
            // Both suspended, reconcile children into detached_parent, fallback into the DOM
            (true, Some(fallback)) => {
                children.reconcile_node(
                    root,
                    parent_scope,
                    &suspense.detached_parent,
                    DomSlot::at_end(),
                    children_bundle,
                );

                let Fallback::Bundle(bundle) = fallback;
                vfallback.reconcile_node(root, parent_scope, parent, slot, bundle)
            }
            // Not suspended, just reconcile the children into the DOM
            (false, None) => {
                children.reconcile_node(root, parent_scope, parent, slot, children_bundle)
            }
            // Freshly suspended. Shift children into the detached parent, then add fallback to the
            // DOM
            (true, None) => {
                children_bundle.shift(&suspense.detached_parent, DomSlot::at_end());

                children.reconcile_node(
                    root,
                    parent_scope,
                    &suspense.detached_parent,
                    DomSlot::at_end(),
                    children_bundle,
                );
                // first render of fallback

                let (fallback_ref, fallback) = vfallback.attach(root, parent_scope, parent, slot);
                suspense.fallback = Some(Fallback::Bundle(fallback));
                fallback_ref
            }
            // Freshly unsuspended. Detach fallback from the DOM, then shift children into it.
            (false, Some(_)) => {
                match suspense.fallback.take() {
                    Some(Fallback::Bundle(bundle)) => {
                        bundle.detach(root, parent, false);
                    }
                    None => {
                        unreachable!("None condition has been checked before.")
                    }
                };

                children_bundle.shift(parent, slot.clone());
                children.reconcile_node(root, parent_scope, parent, slot, children_bundle)
            }
        }
    }
}

