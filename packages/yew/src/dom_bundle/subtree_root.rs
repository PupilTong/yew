//! Per-subtree state of apps.
//!
//! Upstream yew installs a `Closure`-wrapped handler on each subtree root via
//! `addEventListener(capture: true)` and walks the `composed_path()` to route
//! events across portals and shadow boundaries. That guest-side delegation —
//! the bubbling walk, branding lookup, and handler routing — has been removed:
//! the WAMR host owns native dispatch and delivers events directly to a
//! listener's callback rather than bubbling them through the guest.
//!
//! What remains here is the listener *registration* scaffolding that
//! reconciliation still maintains:
//!
//! * `SubtreeData` / `BSubtree` / `ParentingInformation` — the portal mount / parenting data model,
//!   used while mounting controlled subtrees.
//! * `Registry` (in `btag/listeners.rs`) — tracks which listeners are attached where, ready to be
//!   wired to host callbacks once the runtime boundary provides a real callback handle.
//! * `SUBTREE_IDS` — element branding, retained as part of that scaffolding.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU32, Ordering};

use lynx_sys::Element;

use super::Registry;
use crate::virtual_dom::{Listener, ListenerKind};

thread_local! {
    /// Replaces the upstream `__yew_subtree_id` duck-typed property. Keyed directly
    /// on the host reference.
    static SUBTREE_IDS: RefCell<HashMap<i32, TreeId>> = RefCell::new(HashMap::new());
}

fn set_subtree_id(el: i32, tree_id: TreeId) {
    SUBTREE_IDS.with(|m| {
        m.borrow_mut().insert(el, tree_id);
    });
}

fn forget_subtree_id(el: i32) {
    SUBTREE_IDS.with(|m| {
        m.borrow_mut().remove(&el);
    });
}

/// The TreeId is the additional payload attached to each listening element.
/// It identifies the host responsible for the target. Events not matching
/// are ignored during handling.
type TreeId = u32;

static NEXT_ROOT_ID: AtomicU32 = AtomicU32::new(1);

fn next_root_id() -> TreeId {
    NEXT_ROOT_ID.fetch_add(1, Ordering::SeqCst)
}

/// Data kept per controlled subtree. `BPortal` and `AppHandle` serve as
/// hosts. Two controlled subtrees should never overlap.
#[derive(Debug, Clone)]
pub struct BSubtree(Rc<SubtreeData>);

/// Links a controlled subtree to the subtree it is logically mounted under, so
/// that portals share a single app-wide `AppData` with their parent.
#[derive(Debug)]
struct ParentingInformation {
    parent_root: Rc<SubtreeData>,
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

impl EventDescriptor {
    pub(crate) fn event_type(&self) -> Cow<'static, str> {
        self.kind.type_name()
    }

    pub(crate) fn listener_options(&self) -> lynx_sys::EventListenerOptions {
        let options = lynx_sys::EventListenerOptions::new();
        if self.passive {
            options.passive()
        } else {
            options
        }
    }
}

/// Manages host-side event listener registrations for a single subtree.
///
/// Each subtree tracks at most one host-side listener per event kind
/// (delegation model). The actual callback handle must come from the
/// compiler/runtime boundary; this module does not synthesize guest refs.
#[derive(Debug)]
struct HostHandlers {
    /// Shared handle to the host element where events are registered.
    host: Rc<Element>,
    /// Event types already forwarded to the host for this subtree.
    registered: HashSet<ListenerKind>,
}

impl HostHandlers {
    fn new(host: Rc<Element>) -> Self {
        Self {
            host,
            registered: HashSet::new(),
        }
    }

    /// Register a host-side event listener for the given event kind if
    /// one has not already been registered on this subtree's root.
    fn add_listener(&mut self, _subtree: &Rc<SubtreeData>, desc: &EventDescriptor) {
        if self.registered.insert(desc.kind.clone()) {
            let event_type = desc.kind.type_name();
            tracing::debug!(
                host = ?self.host.id(),
                %event_type,
                passive = desc.passive,
                "event listener needs a callback handle from the wasm runtime"
            );
        }
    }

    /// Remove all host-side listeners and clean up the dispatch table.
    fn cleanup(&mut self) {}
}

impl Drop for HostHandlers {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Per subtree data
#[derive(Debug)]
struct SubtreeData {
    /// Data shared between all trees in an app
    app_data: Rc<RefCell<AppData>>,
    subtree_id: TreeId,
    event_registry: RefCell<Registry>,
    global: RefCell<HostHandlers>,
}

#[derive(Debug)]
struct WeakSubtree {
    subtree_id: TreeId,
    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

impl SubtreeData {
    fn new_ref(host: Rc<Element>, parent: Option<ParentingInformation>) -> Rc<Self> {
        let tree_root_id = next_root_id();
        let event_registry = Registry::new();
        let host_handlers = HostHandlers::new(host);
        let app_data = match parent {
            Some(parent) => parent.parent_root.app_data.clone(),
            None => Rc::default(),
        };
        let subtree = Rc::new(SubtreeData {
            app_data,
            subtree_id: tree_root_id,
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

    fn add_listener(self: &Rc<Self>, desc: &EventDescriptor) {
        self.host_handlers().borrow_mut().add_listener(self, desc);
    }
}

impl BSubtree {
    fn do_create_root(host: Rc<Element>, parent: Option<ParentingInformation>) -> Self {
        let host_id = host.id();
        let shared_inner = SubtreeData::new_ref(host, parent);
        let root = BSubtree(shared_inner);
        root.brand_element(host_id);
        root
    }

    /// Create a bundle root at the specified host element.
    pub fn create_root(host_element: &Rc<Element>) -> Self {
        Self::do_create_root(Rc::clone(host_element), None)
    }

    /// Create a bundle root at the specified host element, logically mounted
    /// under this tree so it shares the app-wide event registration state.
    pub fn create_subroot(&self, host_element: &Rc<Element>) -> Self {
        let parent_information = ParentingInformation {
            parent_root: self.0.clone(),
        };
        Self::do_create_root(Rc::clone(host_element), Some(parent_information))
    }

    /// Ensure the event described is handled on all subtrees
    #[allow(dead_code)]
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

    /// Remove subtree branding from a node that is being detached. This prevents
    /// stale entries in `SUBTREE_IDS` from misrouting work after a host
    /// reference becomes stale.
    pub fn unbrand_element(&self, el: i32) {
        forget_subtree_id(el);
    }
}
