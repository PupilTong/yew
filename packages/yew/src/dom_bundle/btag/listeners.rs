use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::rc::Rc;

use rust_wasm_binding::{raw, Element, ExternRef};

use super::Apply;
use crate::dom_bundle::{test_log, BSubtree, EventDescriptor, HostElementListenerRegistration};
use crate::virtual_dom::{Listener, Listeners};

thread_local! {
    /// Duck-typed properties in upstream yew (`__yew_listener_id`) are
    /// replaced with listener metadata keyed by Lynx element unique id.
    /// Entries are cleaned up explicitly from [`ListenerRegistration`] detach
    /// paths; there is no GC backstop.
    static LISTENER_IDS: RefCell<HashMap<ElementKey, u32>> = RefCell::new(HashMap::new());
}

type ElementKey = i64;

fn element_key(el: ExternRef) -> Option<ElementKey> {
    (!el.is_null()).then(|| raw::get_element_unique_id(el))
}

#[allow(dead_code)]
fn take_listener_id(el: ExternRef) -> Option<u32> {
    let key = element_key(el)?;
    LISTENER_IDS.with(|map| map.borrow().get(&key).copied())
}

fn store_listener_id(el: ExternRef, id: u32) {
    let Some(key) = element_key(el) else {
        return;
    };
    LISTENER_IDS.with(|map| {
        map.borrow_mut().insert(key, id);
    });
}

fn forget_listener_id(el: ExternRef) {
    let Some(key) = element_key(el) else {
        return;
    };
    LISTENER_IDS.with(|map| {
        map.borrow_mut().remove(&key);
    });
}

/// DOM-Types that can have listeners registered on them.
#[allow(dead_code)]
pub trait EventListening {
    fn listener_id(&self) -> Option<u32>;
}

impl EventListening for ExternRef {
    fn listener_id(&self) -> Option<u32> {
        take_listener_id(*self)
    }
}

/// An active set of listeners on an element
#[derive(Debug)]
pub(super) enum ListenerRegistration {
    /// No listeners registered.
    NoReg,
    /// Added to global registry by ID, along with the element reference used
    /// for cleanup from [`LISTENER_IDS`].
    Registered {
        id: u32,
        el: ExternRef,
        host_listeners: Vec<HostElementListenerRegistration>,
    },
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

        let current_el = el.id();
        match (self, bundle) {
            (
                Pending(pending),
                Registered {
                    id,
                    el: registered_el,
                    host_listeners,
                },
            ) => {
                // Reuse the ID
                test_log!("reusing listeners for {}", id);
                root.with_listener_registry(|reg| reg.patch(id, &pending));
                if *registered_el != current_el {
                    for host_listener in host_listeners.drain(..) {
                        root.unregister_host_event_listener(&host_listener);
                    }
                    forget_listener_id(*registered_el);
                    store_listener_id(current_el, *id);
                    *registered_el = current_el;
                }
                ListenerRegistration::patch_host_listeners(
                    root,
                    current_el,
                    host_listeners,
                    &pending,
                );
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
                    ListenerRegistration::Registered {
                        id,
                        el,
                        host_listeners,
                    } => {
                        for host_listener in host_listeners.iter() {
                            root.unregister_host_event_listener(host_listener);
                        }
                        (*id, *el)
                    }
                    _ => unreachable!(),
                };
                test_log!("unregistering listeners for {}", id);
                root.with_listener_registry(|reg| reg.unregister(&id));
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
            let id = reg.set_listener_id(el);
            reg.register(id, pending);
            id
        });
        let host_listeners = Self::host_listeners(root, el_id, pending);
        Self::Registered {
            id,
            el: el_id,
            host_listeners,
        }
    }

    /// Remove any registered event listeners from the global registry
    pub fn unregister(&self, root: &BSubtree) {
        if let Self::Registered {
            id,
            el,
            host_listeners,
        } = self
        {
            for host_listener in host_listeners {
                root.unregister_host_event_listener(host_listener);
            }
            root.with_listener_registry(|r| r.unregister(id));
            forget_listener_id(*el);
        }
    }

    fn descriptors(pending: &[Option<Rc<dyn Listener>>]) -> Vec<EventDescriptor> {
        let mut seen = HashSet::<EventDescriptor>::new();
        pending
            .iter()
            .filter_map(|listener| listener.as_ref())
            .filter_map(|listener| {
                let desc = EventDescriptor::from(listener.deref());
                seen.insert(desc.clone()).then_some(desc)
            })
            .collect()
    }

    fn host_listeners(
        root: &BSubtree,
        el: ExternRef,
        pending: &[Option<Rc<dyn Listener>>],
    ) -> Vec<HostElementListenerRegistration> {
        Self::descriptors(pending)
            .into_iter()
            .filter_map(|desc| root.register_host_event_listener(el, &desc))
            .collect()
    }

    fn patch_host_listeners(
        root: &BSubtree,
        el: ExternRef,
        host_listeners: &mut Vec<HostElementListenerRegistration>,
        pending: &[Option<Rc<dyn Listener>>],
    ) {
        let desired = Self::descriptors(pending);
        let desired_set = desired.iter().cloned().collect::<HashSet<_>>();

        let mut index = 0;
        while index < host_listeners.len() {
            if desired_set.contains(&host_listeners[index].desc) {
                index += 1;
            } else {
                let removed = host_listeners.remove(index);
                root.unregister_host_event_listener(&removed);
            }
        }

        let existing = host_listeners
            .iter()
            .map(|registration| registration.desc.clone())
            .collect::<HashSet<_>>();
        for desc in desired {
            if existing.contains(&desc) {
                continue;
            }
            if let Some(registration) = root.register_host_event_listener(el, &desc) {
                host_listeners.push(registration);
            }
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

    /// Handle a single event, given the listening element and event descriptor.
    #[allow(dead_code)]
    pub fn get_handler(
        registry: &RefCell<Registry>,
        listening: &dyn EventListening,
        desc: &EventDescriptor,
    ) -> Option<impl FnOnce()> {
        // The tricky part is that we want to drop the reference to the registry before
        // calling any actual listeners (since that might end up running lifecycle methods
        // and modify the registry). So we clone the current listeners and return a closure
        let listener_id = listening.listener_id()?;
        let registry_ref = registry.borrow();
        let handlers = registry_ref.by_id.get(&listener_id)?;
        let listeners = handlers.get(desc)?.clone();
        drop(registry_ref); // unborrow the registry, before running any listeners
        Some(move || {
            for l in listeners {
                l.handle(());
            }
        })
    }

    /// Register all passed listeners under ID
    fn register(&mut self, id: u32, listeners: &[Option<Rc<dyn Listener>>]) {
        let mut by_desc =
            HashMap::<EventDescriptor, Vec<Rc<dyn Listener>>>::with_capacity(listeners.len());
        for l in listeners.iter().filter_map(|l| l.as_ref()).cloned() {
            let desc = EventDescriptor::from(l.deref());
            by_desc.entry(desc).or_default().push(l);
        }
        self.by_id.insert(id, by_desc);
    }

    /// Patch an already registered set of handlers
    fn patch(&mut self, id: &u32, listeners: &[Option<Rc<dyn Listener>>]) {
        if let Some(by_desc) = self.by_id.get_mut(id) {
            // Keeping empty vectors is fine. Those don't do much and should happen rarely.
            for v in by_desc.values_mut() {
                v.clear()
            }

            for l in listeners.iter().filter_map(|l| l.as_ref()).cloned() {
                let desc = EventDescriptor::from(l.deref());
                by_desc.entry(desc).or_default().push(l);
            }
        }
    }

    /// Unregister any existing listeners for ID
    fn unregister(&mut self, id: &u32) {
        self.by_id.remove(id);
    }

    /// Set unique listener ID for the given element and return it.
    fn set_listener_id(&mut self, el: &Element) -> u32 {
        let id = self.id_counter;
        self.id_counter += 1;

        let el_id = el.id();
        store_listener_id(el_id, id);

        id
    }
}
