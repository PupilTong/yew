#[cfg(all(test, target_arch = "wasm32", verbose_tests))]
macro_rules! test_log {
    ($fmt:literal, $($arg:expr),* $(,)?) => {
        ::wasm_bindgen_test::console_log!(concat!("\t  ", $fmt), $($arg),*);
    };
}
#[cfg(not(all(test, target_arch = "wasm32", verbose_tests)))]
macro_rules! test_log {
    ($fmt:literal, $($arg:expr),* $(,)?) => {
        // Only type-check the format expression, do not run any side effects
        let _ = || { std::format_args!(concat!("\t  ", $fmt), $($arg),*); };
    };
}
/// Log an operation during tests for debugging purposes
/// Set RUSTFLAGS="--cfg verbose_tests" environment variable to activate.
pub(super) use test_log;

#[cfg(test)]
// this is needed because clippy doesn't like the import not being used
#[allow(unused_imports)]
pub(super) use tests::*;

#[cfg(test)]
mod tests {
    #![allow(dead_code)]

    use gloo::utils::document;
    use web_sys::Element;

    use crate::dom_bundle::{BSubtree, DomSlot};
    use crate::html::AnyScope;
    use crate::virtual_dom::vtag::SVG_NAMESPACE;

    pub fn setup_parent() -> (BSubtree, AnyScope, Element) {
        let scope = AnyScope::test();
        let parent = document().create_element("div").unwrap();
        let root = BSubtree::create_root(&parent);

        document().body().unwrap().append_child(&parent).unwrap();

        (root, scope, parent)
    }

    pub fn setup_parent_svg() -> (BSubtree, AnyScope, Element) {
        let scope = AnyScope::test();
        let parent = document()
            .create_element_ns(Some(SVG_NAMESPACE), "svg")
            .unwrap();
        let root = BSubtree::create_root(&parent);

        document().body().unwrap().append_child(&parent).unwrap();

        (root, scope, parent)
    }

    pub const SIBLING_CONTENT: &str = "END";

    pub(crate) fn setup_parent_and_sibling() -> (BSubtree, AnyScope, Element, DomSlot) {
        let scope = AnyScope::test();
        let parent = document().create_element("div").unwrap();
        let root = BSubtree::create_root(&parent);

        document().body().unwrap().append_child(&parent).unwrap();

        let end = document().create_text_node(SIBLING_CONTENT);
        parent.append_child(&end).unwrap();
        let sibling = DomSlot::at(end.into());

        (root, scope, parent, sibling)
    }
}
