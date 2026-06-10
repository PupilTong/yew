use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::rc::Rc;

use lynx_sys::Element;
#[cfg(target_arch = "wasm32")]
use lynx_sys::Event;

use super::Apply;
use crate::dom_bundle::{test_log, BSubtree, EventDescriptor};
use crate::virtual_dom::{Listener, Listeners};

thread_local! {
    /// Duck-typed properties in upstream yew (`__yew_listener_id`) are
    /// replaced with listener metadata keyed directly on the element unique id.
    /// Entries are cleaned up explicitly from [`ListenerRegistration`] detach
    /// paths; there is no GC backstop.
    static LISTENER_IDS: RefCell<HashMap<i64, u32>> = RefCell::new(HashMap::new());
    static EVENT_LISTENERS: RefCell<HashMap<u32, HashMap<EventDescriptor, Vec<Rc<dyn Listener>>>>> =
        RefCell::new(HashMap::new());
}

fn element_unique_id(el: &Element) -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        el.unique_id()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        el.id() as i64
    }
}

fn raw_element_unique_id(el: i32) -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        lynx_sys::raw::get_element_unique_id(el)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        el as i64
    }
}

fn store_listener_id(el: &Element, id: u32) {
    let unique_id = element_unique_id(el);
    LISTENER_IDS.with(|map| {
        map.borrow_mut().insert(unique_id, id);
    });
}

fn forget_listener_id(el: i32) {
    let unique_id = raw_element_unique_id(el);
    LISTENER_IDS.with(|map| {
        map.borrow_mut().remove(&unique_id);
    });
}

#[cfg(target_arch = "wasm32")]
fn event_handler_id() -> i32 {
    __yew_wamr_event_dispatch as usize as i32
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
extern "C" fn __yew_wamr_event_dispatch(event_id: i32) {
    let Some(event) = Event::from_raw(event_id) else {
        return;
    };
    let Some(current_target) = lynx_sys::event_current_target_unique_id(event.raw()) else {
        return;
    };
    let Some(event_type) = lynx_sys::event_type(event.raw()).ok().flatten() else {
        return;
    };

    for listener in listeners_for_event(current_target, &event_type) {
        listener.handle(());
    }
    crate::scheduler::start_now();
}

#[cfg(target_arch = "wasm32")]
fn listeners_for_event(current_target: i64, event_type: &str) -> Vec<Rc<dyn Listener>> {
    let Some(listener_id) = LISTENER_IDS.with(|map| map.borrow().get(&current_target).copied())
    else {
        return Vec::new();
    };
    EVENT_LISTENERS.with(|listeners| {
        listeners
            .borrow()
            .get(&listener_id)
            .and_then(|by_desc| {
                by_desc
                    .iter()
                    .find(|(desc, _)| desc.event_type().as_ref() == event_type)
                    .map(|(_, listeners)| listeners.clone())
            })
            .unwrap_or_default()
    })
}

#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
fn attach_host_listener(el: i32, desc: &EventDescriptor) {
    #[cfg(target_arch = "wasm32")]
    {
        let event_type = desc.event_type();
        let _ = lynx_sys::add_event_listener(
            el,
            event_type.as_ref(),
            event_handler_id(),
            desc.listener_options(),
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
fn detach_host_listener(el: i32, desc: &EventDescriptor) {
    #[cfg(target_arch = "wasm32")]
    {
        let event_type = desc.event_type();
        let _ = lynx_sys::remove_event_listener_with_options(
            el,
            event_type.as_ref(),
            event_handler_id(),
            desc.listener_options(),
        );
    }
}

fn store_event_listeners(id: u32, listeners: HashMap<EventDescriptor, Vec<Rc<dyn Listener>>>) {
    EVENT_LISTENERS.with(|map| {
        map.borrow_mut().insert(id, listeners);
    });
}

fn remove_event_listeners(id: &u32) {
    EVENT_LISTENERS.with(|map| {
        map.borrow_mut().remove(id);
    });
}

/// An active set of listeners on an element
#[derive(Debug)]
pub(super) enum ListenerRegistration {
    /// No listeners registered.
    NoReg,
    /// Added to global registry by ID, along with the element reference used
    /// for cleanup from [`LISTENER_IDS`].
    Registered { id: u32, el: i32 },
}

impl Apply for Listeners {
    type Bundle = ListenerRegistration;

    fn apply(self, root: &BSubtree, el: &Element) -> ListenerRegistration {
        match self {
            Self::Pending(pending) => ListenerRegistration::register(root, el, &pending),
            Self::None => ListenerRegistration::NoReg,
        }
    }

    fn apply_diff(self, root: &BSubtree, el: &Element, bundle: &mut ListenerRegistration) {
        use ListenerRegistration::*;
        use Listeners::*;

        match (self, bundle) {
            (Pending(pending), Registered { id, .. }) => {
                // Reuse the ID
                test_log!("reusing listeners for {}", id);
                root.with_listener_registry(|reg| reg.patch(id, el, &pending));
            }
            (Pending(pending), bundle @ NoReg) => {
                *bundle = ListenerRegistration::register(root, el, &pending);
                test_log!(
                    "registering listeners for {}",
                    match bundle {
                        ListenerRegistration::Registered { id, .. } => id,
                        _ => unreachable!(),
                    }
                );
            }
            (None, bundle @ Registered { .. }) => {
                let (id, el_id) = match bundle {
                    ListenerRegistration::Registered { id, el } => (*id, *el),
                    _ => unreachable!(),
                };
                test_log!("unregistering listeners for {}", id);
                root.with_listener_registry(|reg| reg.unregister(&id, el_id));
                forget_listener_id(el_id);
                *bundle = NoReg;
            }
            (None, NoReg) => {
                test_log!("{}", &"unchanged empty listeners");
            }
        };
    }
}

impl ListenerRegistration {
    /// Register listeners and return their handle ID.
    fn register(root: &BSubtree, el: &Element, pending: &[Option<Rc<dyn Listener>>]) -> Self {
        let el_id = el.id();
        let id = root.with_listener_registry(|reg| {
            let id = reg.set_listener_id(root, el);
            reg.register(id, el, pending);
            id
        });
        Self::Registered { id, el: el_id }
    }

    /// Remove any registered event listeners from the global registry
    pub fn unregister(&self, root: &BSubtree) {
        if let Self::Registered { id, el } = self {
            root.with_listener_registry(|r| r.unregister(id, *el));
            forget_listener_id(*el);
        }
    }
}

/// Global multiplexing event handler registry
#[derive(Debug)]
pub struct Registry {
    /// Counter for assigning new IDs
    id_counter: u32,

    /// Contains all registered event listeners by listener ID
    by_id: HashMap<u32, HashMap<EventDescriptor, Vec<Rc<dyn Listener>>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            id_counter: u32::default(),
            by_id: HashMap::default(),
        }
    }

    /// Register all passed listeners under ID
    fn register(&mut self, id: u32, el: &Element, listeners: &[Option<Rc<dyn Listener>>]) {
        let mut by_desc =
            HashMap::<EventDescriptor, Vec<Rc<dyn Listener>>>::with_capacity(listeners.len());
        for l in listeners.iter().filter_map(|l| l.as_ref()).cloned() {
            let desc = EventDescriptor::from(l.deref());
            by_desc.entry(desc).or_default().push(l);
        }
        for desc in by_desc.keys() {
            attach_host_listener(el.id(), desc);
        }
        store_event_listeners(id, by_desc.clone());
        self.by_id.insert(id, by_desc);
    }

    /// Patch an already registered set of handlers
    fn patch(&mut self, id: &u32, el: &Element, listeners: &[Option<Rc<dyn Listener>>]) {
        if let Some(by_desc) = self.by_id.get_mut(id) {
            let old_descs = by_desc.keys().cloned().collect::<HashSet<_>>();
            let mut next_by_desc =
                HashMap::<EventDescriptor, Vec<Rc<dyn Listener>>>::with_capacity(listeners.len());

            for l in listeners.iter().filter_map(|l| l.as_ref()).cloned() {
                let desc = EventDescriptor::from(l.deref());
                next_by_desc.entry(desc).or_default().push(l);
            }

            let next_descs = next_by_desc.keys().cloned().collect::<HashSet<_>>();
            for desc in old_descs.difference(&next_descs) {
                detach_host_listener(el.id(), desc);
            }
            for desc in next_descs.difference(&old_descs) {
                attach_host_listener(el.id(), desc);
            }
            *by_desc = next_by_desc.clone();
            store_event_listeners(*id, next_by_desc);
        }
    }

    /// Unregister any existing listeners for ID
    fn unregister(&mut self, id: &u32, el: i32) {
        if let Some(by_desc) = self.by_id.remove(id) {
            for desc in by_desc.keys() {
                detach_host_listener(el, desc);
            }
        }
        remove_event_listeners(id);
    }

    /// Set unique listener ID for the given element and return it.
    fn set_listener_id(&mut self, root: &BSubtree, el: &Element) -> u32 {
        let id = self.id_counter;
        self.id_counter += 1;

        root.brand_element(el.id());
        store_listener_id(el, id);

        id
    }
}
