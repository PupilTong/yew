//! Component lifecycle module

use std::any::Any;
use std::rc::Rc;

#[cfg(feature = "csr")]
use rust_wasm_binding::Element;

use super::scope::{AnyScope, Scope};
use super::BaseComponent;
#[cfg(feature = "csr")]
use crate::dom_bundle::{BSubtree, Bundle, DomSlot, DynamicDomSlot};
use crate::html::{Html, RenderError};
use crate::scheduler::{self, Runnable, Shared};
use crate::suspense::{BaseSuspense, Suspension};
use crate::{Callback, Context, HtmlResult};

pub(crate) enum ComponentRenderState {
    #[cfg(feature = "csr")]
    Render {
        bundle: Bundle,
        root: BSubtree,
        /// Shared handle to the parent element this component is mounted
        /// under. Held as `Rc<Element>` so the component can outlive the
        /// trait method scope and so the parent's enclosing
        /// [BTag](crate::dom_bundle::BTag) (or
        /// [AppHandle](crate::AppHandle)) can keep its own clone of the
        /// `Rc` in parallel.
        parent: Rc<Element>,
        /// The dom position in front of the next sibling.
        /// Gets updated when the bundle in which this component occurs gets re-rendered and is
        /// shared with the children of this component.
        sibling_slot: DynamicDomSlot,
        /// The dom position in front of this component.
        /// Gets updated whenever this component re-renders and is shared with the bundle in which
        /// this component occurs.
        own_slot: DynamicDomSlot,
    },
}

impl std::fmt::Debug for ComponentRenderState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "csr")]
            Self::Render {
                ref bundle,
                root,
                ref parent,
                ref sibling_slot,
                ref own_slot,
            } => f
                .debug_struct("ComponentRenderState::Render")
                .field("bundle", bundle)
                .field("root", root)
                .field("parent", parent)
                .field("sibling_slot", sibling_slot)
                .field("own_slot", own_slot)
                .finish(),
        }
    }
}

#[cfg(feature = "csr")]
impl ComponentRenderState {
    pub(crate) fn shift(&mut self, next_parent: Rc<Element>, next_slot: DomSlot) {
        match self {
            #[cfg(feature = "csr")]
            Self::Render {
                bundle,
                parent,
                sibling_slot,
                ..
            } => {
                *parent = next_parent;
                sibling_slot.reassign(next_slot);
                bundle.shift(parent, sibling_slot.to_position());
            }
        }
    }
}

struct CompStateInner<COMP>
where
    COMP: BaseComponent,
{
    pub(crate) component: COMP,
    pub(crate) context: Context<COMP>,
}

/// A trait to provide common,
/// generic free behaviour across all components to reduce code size.
///
/// Mostly a thin wrapper that passes the context to a component's lifecycle
/// methods.
pub(crate) trait Stateful {
    fn view(&self) -> HtmlResult;
    #[cfg(feature = "csr")]
    fn rendered(&mut self, first_render: bool);
    fn destroy(&mut self);

    fn any_scope(&self) -> AnyScope;

    fn flush_messages(&mut self) -> bool;
    #[cfg(feature = "csr")]
    fn props_changed(&mut self, props: Rc<dyn Any>) -> bool;

    fn as_any(&self) -> &dyn Any;
}

impl<COMP> Stateful for CompStateInner<COMP>
where
    COMP: BaseComponent,
{
    fn view(&self) -> HtmlResult {
        self.component.view(&self.context)
    }

    #[cfg(feature = "csr")]
    fn rendered(&mut self, first_render: bool) {
        self.component.rendered(&self.context, first_render)
    }

    fn destroy(&mut self) {
        self.component.destroy(&self.context);
    }

    fn any_scope(&self) -> AnyScope {
        self.context.link().clone().into()
    }

    fn flush_messages(&mut self) -> bool {
        let mut changed = false;
        for msg in self.context.link().pending_messages.drain() {
            if self.component.update(&self.context, msg) {
                changed = true;
            }
        }
        changed
    }

    #[cfg(feature = "csr")]
    fn props_changed(&mut self, props: Rc<dyn Any>) -> bool {
        let props = match Rc::downcast::<COMP::Properties>(props) {
            Ok(m) => m,
            _ => return false,
        };

        if self.context.props != props {
            let old_props = std::mem::replace(&mut self.context.props, props);
            self.component.changed(&self.context, &old_props)
        } else {
            false
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) struct ComponentState {
    pub(super) inner: Box<dyn Stateful>,

    pub(super) render_state: ComponentRenderState,

    #[cfg(feature = "csr")]
    has_rendered: bool,

    suspension: Option<Suspension>,

    pub(crate) comp_id: usize,
}

impl ComponentState {
    #[tracing::instrument(
        level = tracing::Level::DEBUG,
        name = "create",
        skip_all,
        fields(component.id = scope.id),
    )]
    fn new<COMP: BaseComponent>(
        initial_render_state: ComponentRenderState,
        scope: Scope<COMP>,
        props: Rc<COMP::Properties>,
    ) -> Self {
        let comp_id = scope.id;

        let context = Context { scope, props };

        let inner = Box::new(CompStateInner {
            component: COMP::create(&context),
            context,
        });

        Self {
            inner,
            render_state: initial_render_state,
            suspension: None,

            #[cfg(feature = "csr")]
            has_rendered: false,

            comp_id,
        }
    }

    pub(crate) fn downcast_comp_ref<COMP>(&self) -> Option<&COMP>
    where
        COMP: BaseComponent + 'static,
    {
        self.inner
            .as_any()
            .downcast_ref::<CompStateInner<COMP>>()
            .map(|m| &m.component)
    }

    fn resume_existing_suspension(&mut self) {
        if let Some(m) = self.suspension.take() {
            let comp_scope = self.inner.any_scope();

            let suspense_scope = comp_scope.find_parent_scope::<BaseSuspense>().unwrap();
            BaseSuspense::resume(&suspense_scope, m);
        }
    }
}

pub(crate) struct CreateRunner<COMP: BaseComponent> {
    pub initial_render_state: ComponentRenderState,
    pub props: Rc<COMP::Properties>,
    pub scope: Scope<COMP>,
}

impl<COMP: BaseComponent> Runnable for CreateRunner<COMP> {
    fn run(self: Box<Self>) {
        let mut current_state = self.scope.state.borrow_mut();
        if current_state.is_none() {
            *current_state = Some(ComponentState::new(
                self.initial_render_state,
                self.scope.clone(),
                self.props,
            ));
        }
    }
}

pub(crate) struct UpdateRunner {
    pub state: Shared<Option<ComponentState>>,
}

impl ComponentState {
    #[tracing::instrument(
        level = tracing::Level::DEBUG,
        skip(self),
        fields(component.id = self.comp_id)
    )]
    fn update(&mut self) -> bool {
        let schedule_render = self.inner.flush_messages();
        tracing::trace!(schedule_render);
        schedule_render
    }
}

impl Runnable for UpdateRunner {
    fn run(self: Box<Self>) {
        if let Some(state) = self.state.borrow_mut().as_mut() {
            let schedule_render = state.update();

            if schedule_render {
                scheduler::push_component_render(
                    state.comp_id,
                    Box::new(RenderRunner {
                        state: self.state.clone(),
                    }),
                );
                // Only run from the scheduler, so no need to call `scheduler::start()`
            }
        }
    }
}

pub(crate) struct DestroyRunner {
    pub state: Shared<Option<ComponentState>>,
    pub parent_to_detach: bool,
}

impl ComponentState {
    #[tracing::instrument(
        level = tracing::Level::DEBUG,
        skip(self),
        fields(component.id = self.comp_id)
    )]
    fn destroy(mut self, parent_to_detach: bool) {
        self.inner.destroy();
        self.resume_existing_suspension();

        match self.render_state {
            #[cfg(feature = "csr")]
            ComponentRenderState::Render {
                bundle,
                ref parent,
                ref root,
                ..
            } => {
                bundle.detach(root, parent, parent_to_detach);
            }
        }
    }
}

impl Runnable for DestroyRunner {
    fn run(self: Box<Self>) {
        if let Some(state) = self.state.borrow_mut().take() {
            state.destroy(self.parent_to_detach);
        }
    }
}

pub(crate) struct RenderRunner {
    pub state: Shared<Option<ComponentState>>,
}

impl ComponentState {
    #[tracing::instrument(
        level = tracing::Level::DEBUG,
        skip_all,
        fields(component.id = self.comp_id)
    )]
    fn render(&mut self, shared_state: &Shared<Option<ComponentState>>) {
        let view = self.inner.view();
        tracing::trace!(?view, "render result");
        match view {
            Ok(vnode) => self.commit_render(shared_state, vnode),
            Err(RenderError::Suspended(susp)) => self.suspend(shared_state, susp),
        };
    }

    fn suspend(&mut self, shared_state: &Shared<Option<ComponentState>>, suspension: Suspension) {
        // Currently suspended, we re-use previous root node and send
        // suspension to parent element.

        if suspension.resumed() {
            // schedule a render immediately if suspension is resumed.
            scheduler::push_component_render(
                self.comp_id,
                Box::new(RenderRunner {
                    state: shared_state.clone(),
                }),
            );
        } else {
            // We schedule a render after current suspension is resumed.
            let comp_scope = self.inner.any_scope();

            let suspense_scope = comp_scope
                .find_parent_scope::<BaseSuspense>()
                .expect("To suspend rendering, a <Suspense /> component is required.");

            let comp_id = self.comp_id;
            let shared_state = shared_state.clone();
            suspension.listen(Callback::from(move |_| {
                scheduler::push_component_render(
                    comp_id,
                    Box::new(RenderRunner {
                        state: shared_state.clone(),
                    }),
                );
                scheduler::start();
            }));

            if let Some(ref last_suspension) = self.suspension {
                if &suspension != last_suspension {
                    // We remove previous suspension from the suspense.
                    BaseSuspense::resume(&suspense_scope, last_suspension.clone());
                }
            }
            self.suspension = Some(suspension.clone());

            BaseSuspense::suspend(&suspense_scope, suspension);
        }
    }

    fn commit_render(&mut self, shared_state: &Shared<Option<ComponentState>>, new_vdom: Html) {
        // Currently not suspended, we remove any previous suspension and update
        // normally.
        self.resume_existing_suspension();

        match self.render_state {
            #[cfg(feature = "csr")]
            ComponentRenderState::Render {
                ref mut bundle,
                ref parent,
                ref root,
                ref sibling_slot,
                ref mut own_slot,
                ..
            } => {
                let scope = self.inner.any_scope();

                let new_node_ref =
                    bundle.reconcile(root, &scope, parent, sibling_slot.to_position(), new_vdom);
                own_slot.reassign(new_node_ref);

                let first_render = !self.has_rendered;
                self.has_rendered = true;

                scheduler::push_component_rendered(
                    self.comp_id,
                    Box::new(RenderedRunner {
                        state: shared_state.clone(),
                        first_render,
                    }),
                    first_render,
                );
            }
        };
    }
}

impl Runnable for RenderRunner {
    fn run(self: Box<Self>) {
        let mut state = self.state.borrow_mut();
        let state = match state.as_mut() {
            None => return, // skip for components that have already been destroyed
            Some(state) => state,
        };

        state.render(&self.state);
    }
}

#[cfg(feature = "csr")]
mod feat_csr {
    use super::*;

    pub(crate) struct PropsUpdateRunner {
        pub state: Shared<Option<ComponentState>>,
        pub props: Option<Rc<dyn Any>>,
        pub next_sibling_slot: Option<DomSlot>,
    }

    impl ComponentState {
        #[tracing::instrument(
            level = tracing::Level::DEBUG,
            skip(self),
            fields(component.id = self.comp_id)
        )]
        fn changed(
            &mut self,
            props: Option<Rc<dyn Any>>,
            next_sibling_slot: Option<DomSlot>,
        ) -> bool {
            if let Some(next_sibling_slot) = next_sibling_slot {
                // When components are updated, their siblings were likely also updated
                // We also need to shift the bundle so next sibling will be synced to child
                // components.
                match &mut self.render_state {
                    #[cfg(feature = "csr")]
                    ComponentRenderState::Render { sibling_slot, .. } => {
                        sibling_slot.reassign(next_sibling_slot);
                    }
                }
            }

            let should_render = |props: Option<Rc<dyn Any>>, state: &mut ComponentState| -> bool {
                props.map(|m| state.inner.props_changed(m)).unwrap_or(false)
            };

            // Only trigger changed if props were changed / next sibling has changed.
            let schedule_render = should_render(props, self);

            tracing::trace!(
                "props_update(has_rendered={} schedule_render={})",
                self.has_rendered,
                schedule_render
            );
            schedule_render
        }
    }

    impl Runnable for PropsUpdateRunner {
        fn run(self: Box<Self>) {
            let Self {
                next_sibling_slot,
                props,
                state: shared_state,
            } = *self;

            if let Some(state) = shared_state.borrow_mut().as_mut() {
                let schedule_render = state.changed(props, next_sibling_slot);

                if schedule_render {
                    scheduler::push_component_render(
                        state.comp_id,
                        Box::new(RenderRunner {
                            state: shared_state.clone(),
                        }),
                    );
                    // Only run from the scheduler, so no need to call `scheduler::start()`
                }
            };
        }
    }

    pub(crate) struct RenderedRunner {
        pub state: Shared<Option<ComponentState>>,
        pub first_render: bool,
    }

    impl ComponentState {
        #[tracing::instrument(
            level = tracing::Level::DEBUG,
            skip(self),
            fields(component.id = self.comp_id)
        )]
        fn rendered(&mut self, first_render: bool) -> bool {
            if self.suspension.is_none() {
                self.inner.rendered(first_render);
            }

            false
        }
    }

    impl Runnable for RenderedRunner {
        fn run(self: Box<Self>) {
            if let Some(state) = self.state.borrow_mut().as_mut() {
                let has_pending_props = state.rendered(self.first_render);

                if has_pending_props {
                    scheduler::push_component_props_update(Box::new(PropsUpdateRunner {
                        state: self.state.clone(),
                        props: None,
                        next_sibling_slot: None,
                    }));
                }
            }
        }
    }
}

#[cfg(feature = "csr")]
pub(super) use feat_csr::*;
