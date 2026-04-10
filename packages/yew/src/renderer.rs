use std::cell::Cell;
use std::panic::PanicHookInfo as PanicInfo;
use std::rc::Rc;

#[cfg(feature = "csr")]
use rust_wasm_binding::Element;

use crate::app_handle::AppHandle;
use crate::html::BaseComponent;

thread_local! {
    static PANIC_HOOK_IS_SET: Cell<bool> = const { Cell::new(false) };
}

/// Set a custom panic hook.
///
/// In the Paws fork the default panic hook is the Rust standard one (no
/// JS bridge), so this helper is only useful if a host wants to inject its
/// own structured panic handler.
#[cfg(feature = "csr")]
#[allow(clippy::incompatible_msrv)]
pub fn set_custom_panic_hook(hook: Box<dyn Fn(&PanicInfo<'_>) + Sync + Send + 'static>) {
    std::panic::set_hook(hook);
    PANIC_HOOK_IS_SET.with(|hook_is_set| hook_is_set.set(true));
}

/// The Yew Renderer.
///
/// This is the main entry point of a Yew application.
///
/// Unlike upstream yew the Paws fork does not fall back to
/// `document.body()` — there is no browser document to query. Callers
/// always pass an `Rc<Element>` host they own (so the Paws slab entry stays
/// alive as long as the renderer references it).
#[cfg(feature = "csr")]
#[derive(Debug)]
#[must_use = "Renderer does nothing unless render() is called."]
pub struct Renderer<COMP>
where
    COMP: BaseComponent + 'static,
{
    /// Shared handle to the host element. The renderer keeps an `Rc` so
    /// the Paws slab id remains valid for the entire app lifetime.
    root: Rc<Element>,
    props: COMP::Properties,
}

#[cfg(feature = "csr")]
impl<COMP> Renderer<COMP>
where
    COMP: BaseComponent + 'static,
    COMP::Properties: Default,
{
    /// Creates a [Renderer] that renders into a custom root with default properties.
    pub fn with_root(root: Rc<Element>) -> Self {
        Self::with_root_and_props(root, Default::default())
    }
}

#[cfg(feature = "csr")]
impl<COMP> Renderer<COMP>
where
    COMP: BaseComponent + 'static,
{
    /// Creates a [Renderer] that renders into a custom root with custom properties.
    pub fn with_root_and_props(root: Rc<Element>, props: COMP::Properties) -> Self {
        Self { root, props }
    }

    /// Renders the application.
    pub fn render(self) -> AppHandle<COMP> {
        AppHandle::<COMP>::mount_with_props(self.root, Rc::new(self.props))
    }
}
