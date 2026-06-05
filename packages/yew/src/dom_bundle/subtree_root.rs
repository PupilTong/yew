//! Per-subtree state for Lynx/WAMR rendering.
//!
//! Lynx/WAMR registers each listening element with the host directly. The host
//! calls back with the exact listener id that should run.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU32, Ordering};

use rust_wasm_binding::{raw, Element, EventListenerOptions, ExternRef, HostValue};

use super::Registry;
use crate::virtual_dom::{Listener, ListenerKind};

thread_local! {
    static HOST_EVENT_DISPATCH: RefCell<HashMap<u32, HostEventDispatch>> =
        RefCell::new(HashMap::new());
}

type TreeId = u32;

static NEXT_ROOT_ID: AtomicU32 = AtomicU32::new(1);
static NEXT_HOST_EVENT_LISTENER_ID: AtomicU32 = AtomicU32::new(1);

fn next_root_id() -> TreeId {
    NEXT_ROOT_ID.fetch_add(1, Ordering::SeqCst)
}

fn next_host_event_listener_id() -> u32 {
    NEXT_HOST_EVENT_LISTENER_ID.fetch_add(1, Ordering::SeqCst)
}

/// Data kept per controlled subtree. `BPortal` and `AppHandle` serve as hosts.
#[derive(Debug, Clone)]
pub struct BSubtree(Rc<SubtreeData>);

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

#[derive(Clone, Debug)]
struct HostEventDispatch {
    subtree_id: TreeId,
    subtree: Weak<SubtreeData>,
    element: ExternRef,
    desc: EventDescriptor,
}

fn store_host_event_dispatch(listener_id: u32, dispatch: HostEventDispatch) {
    HOST_EVENT_DISPATCH.with(|m| {
        m.borrow_mut().insert(listener_id, dispatch);
    });
}

fn take_host_event_dispatch(listener_id: u32) -> Option<HostEventDispatch> {
    HOST_EVENT_DISPATCH.with(|m| m.borrow().get(&listener_id).cloned())
}

fn forget_host_event_dispatch(listener_id: u32) {
    HOST_EVENT_DISPATCH.with(|m| {
        m.borrow_mut().remove(&listener_id);
    });
}

extern "C" fn host_event_trampoline(listener_id: i32, _target: ExternRef) {
    if listener_id <= 0 {
        return;
    }
    let Some(dispatch) = take_host_event_dispatch(listener_id as u32) else {
        return;
    };
    let Some(subtree) = dispatch.subtree.upgrade() else {
        forget_host_event_dispatch(listener_id as u32);
        return;
    };
    if subtree.subtree_id != dispatch.subtree_id {
        return;
    }
    subtree.handle_current_target(dispatch.desc, dispatch.element);
}

#[derive(Clone, Debug)]
pub(crate) struct HostElementListenerRegistration {
    element: ExternRef,
    id: u32,
    pub(crate) desc: EventDescriptor,
}

/// Per subtree data.
#[derive(Debug)]
struct SubtreeData {
    subtree_id: TreeId,
    event_registry: RefCell<Registry>,
}

impl SubtreeData {
    fn new_ref(_host: Rc<Element>) -> Rc<Self> {
        Rc::new(SubtreeData {
            subtree_id: next_root_id(),
            event_registry: RefCell::new(Registry::new()),
        })
    }

    fn event_registry(&self) -> &RefCell<Registry> {
        &self.event_registry
    }

    fn handle_current_target(&self, desc: EventDescriptor, element: ExternRef) {
        if let Some(handler) = Registry::get_handler(self.event_registry(), &element, &desc) {
            handler();
        }
    }
}

impl BSubtree {
    fn do_create_root(host: Rc<Element>) -> Self {
        BSubtree(SubtreeData::new_ref(host))
    }

    /// Create a bundle root at the specified host element.
    pub fn create_root(host_element: &Rc<Element>) -> Self {
        Self::do_create_root(Rc::clone(host_element))
    }

    /// Create a bundle root at the specified host element.
    pub fn create_subroot(&self, _mount_point: &Rc<Element>, host_element: &Rc<Element>) -> Self {
        Self::do_create_root(Rc::clone(host_element))
    }

    /// Run f with access to global Registry.
    #[inline]
    pub fn with_listener_registry<R>(&self, f: impl FnOnce(&mut Registry) -> R) -> R {
        f(&mut self.0.event_registry().borrow_mut())
    }

    pub(crate) fn unregister_host_event_listener(
        &self,
        registration: &HostElementListenerRegistration,
    ) {
        let event_type = registration.desc.kind.type_name();
        let options = if registration.desc.passive {
            EventListenerOptions::new().passive()
        } else {
            EventListenerOptions::new()
        };
        let _ = rust_wasm_binding::remove_event_listener(
            registration.element,
            event_type.as_ref(),
            host_event_trampoline,
            registration.id as i32,
            options,
        );
        forget_host_event_dispatch(registration.id);
    }

    pub(crate) fn register_host_event_listener(
        &self,
        element: ExternRef,
        desc: &EventDescriptor,
    ) -> Option<HostElementListenerRegistration> {
        let event_type = desc.kind.type_name();
        let listener_id = next_host_event_listener_id();
        let options = if desc.passive {
            EventListenerOptions::new().passive()
        } else {
            EventListenerOptions::new()
        };
        store_host_event_dispatch(
            listener_id,
            HostEventDispatch {
                subtree_id: self.0.subtree_id,
                subtree: Rc::downgrade(&self.0),
                element,
                desc: desc.clone(),
            },
        );

        raw::set_attribute(
            element,
            HostValue::String("flatten"),
            HostValue::Bool(false),
        );

        if let Err(error) = rust_wasm_binding::add_event_listener(
            element,
            event_type.as_ref(),
            host_event_trampoline,
            listener_id as i32,
            options,
        ) {
            tracing::error!(
                element = ?element,
                %event_type,
                passive = desc.passive,
                %error,
                "failed to register host event listener"
            );
            forget_host_event_dispatch(listener_id);
            return None;
        }
        Some(HostElementListenerRegistration {
            element,
            id: listener_id,
            desc: desc.clone(),
        })
    }
}
