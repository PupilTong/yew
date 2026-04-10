//! Structs for keeping track where in the DOM a node belongs

use std::cell::RefCell;
use std::rc::Rc;

use rust_wasm_binding::{Element, NodeOps};

/// Sentinel node id used by the trap check in debug builds. Real Paws node
/// ids are always `>= 0`, so `i32::MIN` is guaranteed not to collide.
const TRAP_SENTINEL: i32 = i32::MIN;

/// A position in the list of children of an implicit parent element.
///
/// This can either be in front of a `DomSlot::at(next_sibling)`, at the end of the list with
/// `DomSlot::at_end()`, or a dynamic position in the list with [`DynamicDomSlot::to_position`].
#[derive(Clone)]
pub(crate) struct DomSlot {
    variant: DomSlotVariant,
}

#[derive(Clone)]
enum DomSlotVariant {
    /// A next-sibling Paws node id, or `None` for "append at end".
    Node(Option<i32>),
    Chained(DynamicDomSlot),
}

/// A dynamic dom slot can be reassigned. This change is also seen by the [`DomSlot`] from
/// [`Self::to_position`] before the reassignment took place.
#[derive(Clone)]
pub(crate) struct DynamicDomSlot {
    target: Rc<RefCell<DomSlot>>,
}

impl std::fmt::Debug for DomSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.with_next_sibling(|n| {
            let formatted = match n {
                None => None,
                Some(id) if is_trap(id) => Some("<not yet initialized />".to_string()),
                Some(id) => Some(format!("node({id})")),
            };
            write!(f, "DomSlot {{ next_sibling: {formatted:?} }}")
        })
    }
}

impl std::fmt::Debug for DynamicDomSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#?}", *self.target.borrow())
    }
}

#[inline]
fn is_trap(id: i32) -> bool {
    #[cfg(debug_assertions)]
    {
        id == TRAP_SENTINEL
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = id;
        false
    }
}

impl DomSlot {
    /// Denotes the position just before the given node in its parent's list of children.
    pub fn at(next_sibling: i32) -> Self {
        Self::create(Some(next_sibling))
    }

    /// Denotes the position at the end of a list of children. The parent is implicit.
    pub fn at_end() -> Self {
        Self::create(None)
    }

    pub fn create(next_sibling: Option<i32>) -> Self {
        Self {
            variant: DomSlotVariant::Node(next_sibling),
        }
    }

    /// Get the next-sibling Paws node id that comes just after the position, or `None` if this
    /// denotes the position at the end.
    fn with_next_sibling_check_trap<R>(&self, f: impl FnOnce(Option<i32>) -> R) -> R {
        let checkedf = |node: Option<i32>| {
            let is_trapped = matches!(node, Some(id) if is_trap(id));
            assert!(
                !is_trapped,
                "Should not use a trapped DomSlot. Please report this as an internal bug in yew."
            );
            f(node)
        };
        self.with_next_sibling(checkedf)
    }

    fn with_next_sibling<R>(&self, f: impl FnOnce(Option<i32>) -> R) -> R {
        match &self.variant {
            DomSlotVariant::Node(n) => f(*n),
            DomSlotVariant::Chained(chain) => chain.with_next_sibling(f),
        }
    }

    /// Insert `node` at the position denoted by this slot. `parent` must be the actual parent
    /// element of the children that this slot is implicitly a part of.
    pub(super) fn insert(&self, parent: &Element, node: i32) {
        self.with_next_sibling_check_trap(|next_sibling: Option<i32>| {
            // Paws' `insert_before` takes `-1` as the "at end" sentinel.
            let ref_child = next_sibling.unwrap_or(-1);
            if let Err(err) = rust_wasm_binding::insert_before(parent.id(), node, ref_child) {
                let msg = if next_sibling.is_some() {
                    "failed to insert node before next sibling"
                } else {
                    "failed to append child"
                };
                tracing::error!(
                    ?err,
                    parent = parent.id(),
                    next_sibling = ?next_sibling,
                    node,
                    "{msg}"
                );
                panic!("{}", msg)
            }
        });
    }
}

impl DynamicDomSlot {
    /// Create a dynamic dom slot that initially represents ("targets") the same slot as the
    /// argument.
    pub fn new(initial_position: DomSlot) -> Self {
        Self {
            target: Rc::new(RefCell::new(initial_position)),
        }
    }

    /// Change the [`DomSlot`] that is targeted. Subsequently, this will behave as if `self` was
    /// created from the passed DomSlot in the first place.
    pub fn reassign(&self, next_position: DomSlot) {
        // TODO: is not defensive against accidental reference loops
        *self.target.borrow_mut() = next_position;
    }

    /// Get a [`DomSlot`] that gets automatically updated when `self` gets reassigned. All such
    /// slots are equivalent to each other and point to the same position.
    pub fn to_position(&self) -> DomSlot {
        DomSlot {
            variant: DomSlotVariant::Chained(self.clone()),
        }
    }

    fn with_next_sibling<R>(&self, f: impl FnOnce(Option<i32>) -> R) -> R {
        // we use an iterative approach to traverse a possible long chain for references
        // see for example issue #3043 why a recursive call is impossible for large lists in vdom

        // TODO: there could be some data structure that performs better here. E.g. a balanced tree
        // with parent pointers come to mind, but they are a bit fiddly to implement in rust
        let mut this = self.target.clone();
        loop {
            //                          v------- borrow lives for this match expression
            let next_this = match &this.borrow().variant {
                DomSlotVariant::Node(n) => break f(*n),
                // We clone an Rc here temporarily, so that we don't have to consume stack
                // space. The alternative would be to keep the
                // `Ref<'_, DomSlot>` above in some temporary buffer
                DomSlotVariant::Chained(ref chain) => chain.target.clone(),
            };
            this = next_this;
        }
    }
}
