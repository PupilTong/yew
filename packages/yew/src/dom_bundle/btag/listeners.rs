use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::JsCast;
use web_sys::{Element, Event, EventTarget as HtmlEventTarget};

use super::Apply;
use crate::dom_bundle::{test_log, BSubtree, EventDescriptor};
use crate::virtual_dom::{Listener, Listeners};

#[wasm_bindgen]
extern "C" {
    // Duck-typing, not a real class on js-side. On rust-side, use impls of EventTarget below
    type EventTargetable;
    #[wasm_bindgen(method, getter = __yew_listener_id, structural)]
    fn listener_id(this: &EventTargetable) -> Option<u32>;
    #[wasm_bindgen(method, setter = __yew_listener_id, structural)]
    fn set_listener_id(this: &EventTargetable, id: u32);
}

/// DOM-Types that can have listeners registered on them.
/// Uses the duck-typed interface from above in impls.
pub trait EventListening {
    fn listener_id(&self) -> Option<u32>;
    fn set_listener_id(&self, id: u32);
}

impl EventListening for Element {
    fn listener_id(&self) -> Option<u32> {
        self.unchecked_ref::<EventTargetable>().listener_id()
    }

    fn set_listener_id(&self, id: u32) {
        self.unchecked_ref::<EventTargetable>().set_listener_id(id);
    }
}

/// An active set of listeners on an element
#[derive(Debug)]
pub(super) enum ListenerRegistration {
    /// No listeners registered.
    NoReg,
    /// Added to global registry by ID
    Registered(u32),
}

impl Apply for Listeners {
    type Bundle = ListenerRegistration;
    type Element = Element;

    fn apply(self, root: &BSubtree, el: &Self::Element) -> ListenerRegistration {
        match self {
            Self::Pending(pending) => ListenerRegistration::register(root, el, &pending),
            Self::None => ListenerRegistration::NoReg,
        }
    }

    fn apply_diff(self, root: &BSubtree, el: &Self::Element, bundle: &mut ListenerRegistration) {
        use ListenerRegistration::*;
        use Listeners::*;

        match (self, bundle) {
            (Pending(pending), Registered(ref id)) => {
                // Reuse the ID
                test_log!("reusing listeners for {}", id);
                root.with_listener_registry(|reg| reg.patch(root, id, &pending));
            }
            (Pending(pending), bundle @ NoReg) => {
                *bundle = ListenerRegistration::register(root, el, &pending);
                test_log!(
                    "registering listeners for {}",
                    match bundle {
                        ListenerRegistration::Registered(id) => id,
                        _ => unreachable!(),
                    }
                );
            }
            (None, bundle @ Registered(_)) => {
                let id = match bundle {
                    ListenerRegistration::Registered(ref id) => id,
                    _ => unreachable!(),
                };
                test_log!("unregistering listeners for {}", id);
                root.with_listener_registry(|reg| reg.unregister(id));
                *bundle = NoReg;
            }
            (None, NoReg) => {
                test_log!("{}", &"unchanged empty listeners");
            }
        };
    }
}

impl ListenerRegistration {
    /// Register listeners and return their handle ID
    fn register(root: &BSubtree, el: &Element, pending: &[Option<Rc<dyn Listener>>]) -> Self {
        Self::Registered(root.with_listener_registry(|reg| {
            let id = reg.set_listener_id(root, el);
            reg.register(root, id, pending);
            id
        }))
    }

    /// Remove any registered event listeners from the global registry
    pub fn unregister(&self, root: &BSubtree) {
        if let Self::Registered(id) = self {
            root.with_listener_registry(|r| r.unregister(id));
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
    pub fn get_handler(
        registry: &RefCell<Registry>,
        listening: &dyn EventListening,
        desc: &EventDescriptor,
    ) -> Option<impl FnOnce(&Event)> {
        // The tricky part is that we want to drop the reference to the registry before
        // calling any actual listeners (since that might end up running lifecycle methods
        // and modify the registry). So we clone the current listeners and return a closure
        let listener_id = listening.listener_id()?;
        let registry_ref = registry.borrow();
        let handlers = registry_ref.by_id.get(&listener_id)?;
        let listeners = handlers.get(desc)?.clone();
        drop(registry_ref); // unborrow the registry, before running any listeners
        Some(move |event: &Event| {
            for l in listeners {
                l.handle(event.clone());
            }
        })
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

    /// Set unique listener ID onto element and return it
    fn set_listener_id(&mut self, root: &BSubtree, el: &Element) -> u32 {
        let id = self.id_counter;
        self.id_counter += 1;

        root.brand_element(el as &HtmlEventTarget);
        el.set_listener_id(id);

        id
    }
}
