use std::cell::Cell;
use std::panic::PanicHookInfo as PanicInfo;
use std::rc::Rc;

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
/// always pass the Paws node id of the element they want to mount into.
#[cfg(feature = "csr")]
#[derive(Debug)]
#[must_use = "Renderer does nothing unless render() is called."]
pub struct Renderer<COMP>
where
    COMP: BaseComponent + 'static,
{
    /// Paws node id of the host element.
    root: i32,
    props: COMP::Properties,
}

impl<COMP> Renderer<COMP>
where
    COMP: BaseComponent + 'static,
    COMP::Properties: Default,
{
    /// Creates a [Renderer] that renders into a custom root with default properties.
    pub fn with_root(root: i32) -> Self {
        Self::with_root_and_props(root, Default::default())
    }
}

impl<COMP> Renderer<COMP>
where
    COMP: BaseComponent + 'static,
{
    /// Creates a [Renderer] that renders into a custom root with custom properties.
    pub fn with_root_and_props(root: i32, props: COMP::Properties) -> Self {
        Self { root, props }
    }

    /// Renders the application.
    pub fn render(self) -> AppHandle<COMP> {
        AppHandle::<COMP>::mount_with_props(self.root, Rc::new(self.props))
    }
}
