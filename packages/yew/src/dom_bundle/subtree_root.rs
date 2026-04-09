//! Per-subtree state of apps.
//!
//! **Paws fork note.** The browser version of this file installs a
//! `Closure`-wrapped handler on each subtree root via
//! `addEventListener(capture: true)` and manually walks the
//! `composed_path()` to route events across portals and shadow boundaries.
//! None of that machinery exists on Paws — the host (Paws'
//! `wasmtime-engine`) owns the W3C three-phase dispatch and invokes the
//! guest via `__paws_invoke_listener(callback_id)`.
//!
//! The guest-side pieces that *do* remain are:
//!
//! * `SUBTREE_IDS` / `CACHE_KEYS` — guest-side thread-local maps that replace the
//!   `__yew_subtree_id` / `__yew_subtree_cache_key` properties upstream yew attached to DOM nodes
//!   via duck-typed wasm_bindgen.
//! * `SubtreeData` / `BSubtree` / `ParentingInformation` — the portal bubble-path data model is
//!   unchanged; walking the tree still uses [`ParentingInformation`] +
//!   `rust_wasm_binding::get_parent_element`.
//! * `Registry` (in `btag/listeners.rs`) — still the single source of truth for which handlers are
//!   attached where; its lookup path is used by `handle()` when a host dispatch arrives.
//!
//! The actual host → guest callback wiring is a follow-up; today
//! [`HostHandlers::add_listener`] is a no-op stub (see
//! `todo!("Phase 2: …")` comments) so yew compiles and reconciles but
//! listeners never fire. This keeps the Phase 2 diff tractable — the
//! dispatcher-table generation is its own concern.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::{test_log, Registry};
use crate::virtual_dom::{Listener, ListenerKind};

thread_local! {
    /// Replaces the `__yew_subtree_id` duck-typed JS property. Keyed on the
    /// Paws slab id of the element.
    static SUBTREE_IDS: RefCell<HashMap<i32, TreeId>> = RefCell::new(HashMap::new());
    /// Replaces the `__yew_subtree_cache_key` duck-typed JS property (which
    /// upstream yew stored on the `Event` object itself to avoid walking
    /// the composed path more than once per event). On Paws the event
    /// object is transient and owned by the host; we mirror the value by
    /// keyed node id to preserve the existing API shape even though the
    /// cache hit rate is no longer the same.
    static CACHE_KEYS: RefCell<HashMap<i32, u32>> = RefCell::new(HashMap::new());
}

fn get_subtree_id(el: i32) -> Option<TreeId> {
    SUBTREE_IDS.with(|m| m.borrow().get(&el).copied())
}

fn set_subtree_id(el: i32, tree_id: TreeId) {
    SUBTREE_IDS.with(|m| {
        m.borrow_mut().insert(el, tree_id);
    });
}

#[allow(dead_code)]
fn get_cache_key(el: i32) -> Option<u32> {
    CACHE_KEYS.with(|m| m.borrow().get(&el).copied())
}

fn set_cache_key(el: i32, key: u32) {
    CACHE_KEYS.with(|m| {
        m.borrow_mut().insert(el, key);
    });
}

/// The TreeId is the additional payload attached to each listening element.
/// It identifies the host responsible for the target. Events not matching
/// are ignored during handling.
type TreeId = u32;

/// Special id for caching the fact that some event should not be handled
static NONE_TREE_ID: TreeId = 0;
static NEXT_ROOT_ID: AtomicU32 = AtomicU32::new(1);

fn next_root_id() -> TreeId {
    NEXT_ROOT_ID.fetch_add(1, Ordering::SeqCst)
}

/// Data kept per controlled subtree. `BPortal` and `AppHandle` serve as
/// hosts. Two controlled subtrees should never overlap.
#[derive(Debug, Clone)]
pub struct BSubtree(Rc<SubtreeData>);

/// The parent is the logical location where a subtree is mounted.
/// Used to bubble events through portals, which are physically somewhere
/// else in the DOM tree but should bubble to logical ancestors in the
/// virtual DOM tree.
#[derive(Debug)]
struct ParentingInformation {
    parent_root: Rc<SubtreeData>,
    /// Logical parent of the subtree. Might be the host element of another
    /// subtree, if mounted as a direct child, or a controlled element.
    mount_element: i32,
}

#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct EventDescriptor {
    kind: ListenerKind,
    passive: bool,
}

impl From<&dyn Listener> for EventDescriptor {
    fn from(l: &dyn Listener) -> Self {
        Self {
            kind: l.kind(),
            passive: l.passive(),
        }
    }
}

/// Ensures event handler registration.
///
/// **Stub.** The browser version installed `Closure::wrap`ped handlers on
/// the subtree root via `addEventListener`; on Paws the plan is to
/// generate a non-capturing `fn()` dispatcher per [`ListenerKind`] and
/// hand that to [`rust_wasm_binding::register_listener`] +
/// [`rust_wasm_binding::add_event_listener`]. That wiring is not part of
/// Phase 2 yet — once it lands, the per-subtree book-keeping here will be
/// repurposed to drive it.
#[derive(Debug, Default)]
struct HostHandlers {
    /// Host element node id where events are registered
    host: i32,
    /// Event types already forwarded to the host for this subtree.
    registered: HashSet<ListenerKind>,
}

impl HostHandlers {
    fn new(host: i32) -> Self {
        Self {
            host,
            registered: HashSet::new(),
        }
    }

    fn add_listener(&mut self, desc: &EventDescriptor) {
        if self.registered.insert(desc.kind.clone()) {
            // TODO(Paws Phase 2 follow-up): register a per-kind dispatcher
            // via `rust_wasm_binding::register_listener` + `add_event_listener`.
            // Today the listener is tracked guest-side only; actual dispatch
            // is wired in a later commit.
            let _ = self.host;
        }
    }
}

/// Per subtree data
#[derive(Debug)]
struct SubtreeData {
    /// Data shared between all trees in an app
    app_data: Rc<RefCell<AppData>>,
    /// Parent subtree
    parent: Option<ParentingInformation>,

    subtree_id: TreeId,
    /// Paws node id of the host element.
    host: i32,
    event_registry: RefCell<Registry>,
    global: RefCell<HostHandlers>,
}

#[derive(Debug)]
struct WeakSubtree {
    subtree_id: TreeId,
    weak_ref: Weak<SubtreeData>,
}

impl Hash for WeakSubtree {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.subtree_id.hash(state)
    }
}

impl PartialEq for WeakSubtree {
    fn eq(&self, other: &Self) -> bool {
        self.subtree_id == other.subtree_id
    }
}
impl Eq for WeakSubtree {}

/// Per tree data, shared between all subtrees in the hierarchy
#[derive(Debug, Default)]
struct AppData {
    subtrees: HashSet<WeakSubtree>,
    listening: HashSet<EventDescriptor>,
}

impl AppData {
    fn add_subtree(&mut self, subtree: &Rc<SubtreeData>) {
        for event in self.listening.iter() {
            subtree.add_listener(event);
        }
        self.subtrees.insert(WeakSubtree {
            subtree_id: subtree.subtree_id,
            weak_ref: Rc::downgrade(subtree),
        });
    }

    fn ensure_handled(&mut self, desc: &EventDescriptor) {
        if !self.listening.insert(desc.clone()) {
            return;
        }
        self.subtrees.retain(|subtree| {
            if let Some(subtree) = subtree.weak_ref.upgrade() {
                subtree.add_listener(desc);
                true
            } else {
                false
            }
        })
    }
}

/// Bubble events during delegation
static BUBBLE_EVENTS: AtomicBool = AtomicBool::new(true);

/// Set, if events should bubble up the DOM tree, calling any matching callbacks.
///
/// Bubbling is enabled by default. Disabling bubbling can lead to substantial improvements in event
/// handling performance.
///
/// This function should be called before any component is mounted.
#[cfg(feature = "csr")]
pub fn set_event_bubbling(bubble: bool) {
    BUBBLE_EVENTS.store(bubble, Ordering::Relaxed);
}

/// Walk a single step of the bubble path. In the browser this crossed
/// shadow DOM boundaries via `ShadowRoot::host()`; on Paws the guest has
/// no shadow-root awareness, so we just walk physical parents.
fn parent_step(el: i32) -> Option<i32> {
    rust_wasm_binding::get_parent_element(el)
}

struct BrandingSearchResult {
    branding: TreeId,
    closest_branded_ancestor: i32,
}

/// Deduce the subtree an element is part of. This already partially starts
/// the bubbling process, as long as no listeners are encountered.
/// Subtree roots are always branded with their own subtree id.
fn find_closest_branded_element(mut el: i32, do_bubble: bool) -> Option<BrandingSearchResult> {
    if !do_bubble {
        let branding = get_subtree_id(el)?;
        Some(BrandingSearchResult {
            branding,
            closest_branded_ancestor: el,
        })
    } else {
        let responsible_tree_id = loop {
            if let Some(tree_id) = get_subtree_id(el) {
                break tree_id;
            }
            el = parent_step(el)?;
        };
        Some(BrandingSearchResult {
            branding: responsible_tree_id,
            closest_branded_ancestor: el,
        })
    }
}

/// Iterate over all potentially listening elements in bubbling order.
/// If bubbling is turned off, yields at most a single element.
fn start_bubbling_from(
    subtree: &SubtreeData,
    root_or_listener: i32,
    should_bubble: bool,
) -> impl '_ + Iterator<Item = (&'_ SubtreeData, i32)> {
    let start = subtree.bubble_to_inner_element(root_or_listener, should_bubble);

    std::iter::successors(start, move |(subtree, element)| {
        if !should_bubble {
            return None;
        }
        let parent = parent_step(*element)?;
        subtree.bubble_to_inner_element(parent, true)
    })
}

impl SubtreeData {
    fn new_ref(host_element: i32, parent: Option<ParentingInformation>) -> Rc<Self> {
        let tree_root_id = next_root_id();
        let event_registry = Registry::new();
        let host_handlers = HostHandlers::new(host_element);
        let app_data = match parent {
            Some(ref parent) => parent.parent_root.app_data.clone(),
            None => Rc::default(),
        };
        let subtree = Rc::new(SubtreeData {
            parent,
            app_data,

            subtree_id: tree_root_id,
            host: host_element,
            event_registry: RefCell::new(event_registry),
            global: RefCell::new(host_handlers),
        });
        subtree.app_data.borrow_mut().add_subtree(&subtree);
        subtree
    }

    fn event_registry(&self) -> &RefCell<Registry> {
        &self.event_registry
    }

    fn host_handlers(&self) -> &RefCell<HostHandlers> {
        &self.global
    }

    // Bubble a potential parent until it reaches an internal element
    fn bubble_to_inner_element(&self, parent_el: i32, should_bubble: bool) -> Option<(&Self, i32)> {
        let mut next_subtree = self;
        let mut next_el = parent_el;
        if !should_bubble && next_subtree.host == next_el {
            return None;
        }
        while next_subtree.host == next_el {
            // we've reached the host, delegate to a parent if one exists
            let parent = next_subtree.parent.as_ref()?;
            next_subtree = &parent.parent_root;
            next_el = parent.mount_element;
        }
        Some((next_subtree, next_el))
    }

    /// Resolve which subtree is responsible for a host-dispatched event
    /// whose current target is `target`. Parallel to the browser version's
    /// `start_bubbling_if_responsible`, but stripped of the caching path
    /// that relied on mutating the `Event` object.
    #[allow(dead_code)]
    fn start_bubbling_if_responsible<'s>(
        &'s self,
        target: i32,
    ) -> Option<impl 's + Iterator<Item = (&'s SubtreeData, i32)>> {
        let should_bubble = BUBBLE_EVENTS.load(Ordering::Relaxed);
        let (responsible_tree_id, bubbling_start) =
            if let Some(branding) = find_closest_branded_element(target, should_bubble) {
                let BrandingSearchResult {
                    branding,
                    closest_branded_ancestor,
                } = branding;
                // Cache branding on the target element id (mirrors upstream
                // yew's mutation of the Event object).
                set_cache_key(target, branding);
                (branding, closest_branded_ancestor)
            } else {
                set_cache_key(target, NONE_TREE_ID);
                return None;
            };
        if self.subtree_id != responsible_tree_id {
            return None;
        }
        if self.host == target {
            // One more special case: don't handle events that get fired
            // directly on a subtree host.
            return None;
        }
        Some(start_bubbling_from(self, bubbling_start, should_bubble))
    }

    /// Handle a global event firing.
    ///
    /// Stub for Phase 2 — wires up once the host-side dispatcher-table
    /// integration lands. Today this is dead code but kept structurally so
    /// the follow-up diff touches the fewest files possible.
    #[allow(dead_code)]
    fn handle(&self, desc: EventDescriptor, target: i32) {
        let run_handler = |root: &Self, el: i32| {
            let handler = Registry::get_handler(root.event_registry(), &el, &desc);
            if let Some(handler) = handler {
                handler();
            }
        };
        if let Some(bubbling_it) = self.start_bubbling_if_responsible(target) {
            test_log!("Running handler on subtree {}", self.subtree_id);
            for (subtree, el) in bubbling_it {
                // Paws exposes `event_default_prevented()` but not a
                // dedicated cancel_bubble bit; use prevent-default as a
                // stand-in until the dispatcher wiring is fleshed out.
                if rust_wasm_binding::event_default_prevented() {
                    break;
                }
                run_handler(subtree, el);
            }
        }
    }

    fn add_listener(self: &Rc<Self>, desc: &EventDescriptor) {
        self.host_handlers().borrow_mut().add_listener(desc);
    }
}

impl BSubtree {
    fn do_create_root(host_element: i32, parent: Option<ParentingInformation>) -> Self {
        let shared_inner = SubtreeData::new_ref(host_element, parent);
        let root = BSubtree(shared_inner);
        root.brand_element(host_element);
        root
    }

    /// Create a bundle root at the specified host element
    pub fn create_root(host_element: i32) -> Self {
        Self::do_create_root(host_element, None)
    }

    /// Create a bundle root at the specified host element, that is logically
    /// mounted under the specified element in this tree.
    pub fn create_subroot(&self, mount_point: i32, host_element: i32) -> Self {
        let parent_information = ParentingInformation {
            parent_root: self.0.clone(),
            mount_element: mount_point,
        };
        Self::do_create_root(host_element, Some(parent_information))
    }

    /// Ensure the event described is handled on all subtrees
    pub fn ensure_handled(&self, desc: &EventDescriptor) {
        self.0.app_data.borrow_mut().ensure_handled(desc);
    }

    /// Run f with access to global Registry
    #[inline]
    pub fn with_listener_registry<R>(&self, f: impl FnOnce(&mut Registry) -> R) -> R {
        f(&mut self.0.event_registry().borrow_mut())
    }

    pub fn brand_element(&self, el: i32) {
        set_subtree_id(el, self.0.subtree_id);
    }
}
