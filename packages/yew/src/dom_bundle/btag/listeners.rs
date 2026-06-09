use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use lynx_sys::{Element, ExternRef};

use super::Apply;
use crate::dom_bundle::{test_log, BSubtree, EventDescriptor};
use crate::virtual_dom::{Listener, Listeners};

thread_local! {
    /// Duck-typed properties in upstream yew (`__yew_listener_id`) are
    /// replaced with listener metadata keyed directly on the host reference.
    /// Entries are cleaned up explicitly from [`ListenerRegistration`] detach
    /// paths; there is no GC backstop.
    static LISTENER_IDS: RefCell<HashMap<ExternRef, u32>> = RefCell::new(HashMap::new());
}

fn store_listener_id(el: ExternRef, id: u32) {
    LISTENER_IDS.with(|map| {
        map.borrow_mut().insert(el, id);
    });
}

fn forget_listener_id(el: ExternRef) {
    LISTENER_IDS.with(|map| {
        map.borrow_mut().remove(&el);
    });
}

/// An active set of listeners on an element
#[derive(Debug)]
pub(super) enum ListenerRegistration {
    /// No listeners registered.
    NoReg,
    /// Added to global registry by ID, along with the element reference used
    /// for cleanup from [`LISTENER_IDS`].
    Registered { id: u32, el: ExternRef },
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
                root.with_listener_registry(|reg| reg.patch(root, id, &pending));
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
            let id = reg.set_listener_id(root, el);
            reg.register(root, id, pending);
            id
        });
        Self::Registered { id, el: el_id }
    }

    /// Remove any registered event listeners from the global registry
    pub fn unregister(&self, root: &BSubtree) {
        if let Self::Registered { id, el } = self {
            root.with_listener_registry(|r| r.unregister(id));
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
    fn register(&mut self, root: &BSubtree, id: u32, listeners: &[Option<Rc<dyn Listener>>]) {
        let mut by_desc =
            HashMap::<EventDescriptor, Vec<Rc<dyn Listener>>>::with_capacity(listeners.len());
        for l in listeners.iter().filter_map(|l| l.as_ref()).cloned() {
            let desc = EventDescriptor::from(l.deref());
            root.ensure_handled(&desc);
            by_desc.entry(desc).or_default().push(l);
        }
        self.by_id.insert(id, by_desc);
    }

    /// Patch an already registered set of handlers
    fn patch(&mut self, root: &BSubtree, id: &u32, listeners: &[Option<Rc<dyn Listener>>]) {
        if let Some(by_desc) = self.by_id.get_mut(id) {
            // Keeping empty vectors is fine. Those don't do much and should happen rarely.
            for v in by_desc.values_mut() {
                v.clear()
            }

            for l in listeners.iter().filter_map(|l| l.as_ref()).cloned() {
                let desc = EventDescriptor::from(l.deref());
                root.ensure_handled(&desc);
                by_desc.entry(desc).or_default().push(l);
            }
        }
    }

    /// Unregister any existing listeners for ID
    fn unregister(&mut self, id: &u32) {
        self.by_id.remove(id);
    }

    /// Set unique listener ID for the given element and return it.
    fn set_listener_id(&mut self, root: &BSubtree, el: &Element) -> u32 {
        let id = self.id_counter;
        self.id_counter += 1;

        let el_id = el.id();
        root.brand_element(el_id);
        store_listener_id(el_id, id);

        id
    }
}
