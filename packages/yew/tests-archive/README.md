# Tests Archive

This directory contains **the pre-deletion versions** of every test file and
test module that was removed from the yew fork in commit
[`refactor: remove browser-only test code`](#). The goal is to preserve the
tests verbatim so they can be ported to Paws' E2E test harness once the
`web-sys` / `js-sys` / `gloo` / `wasm-bindgen` production code has been
stripped from the yew fork.

**None of the files in this directory are compiled.** Cargo only picks up
`.rs` files that are reachable through `mod` declarations in `src/`, or that
live directly under the magic `tests/` directory. Everything here is inert.

## Why these tests were removed

All the tests in this archive depend on at least one of:

- `wasm_bindgen_test::wasm_bindgen_test` — a browser-based test runner that
  loads the compiled `.wasm` into headless Chrome or Firefox and reports
  results back over stdout.
- `gloo::utils::document()` — accesses the real browser `Document` object.
- `web_sys::Element` / `Node` / `Event` method calls — wrapped bindings
  around browser DOM APIs.

None of these work on Paws, which is a `wasmtime`-hosted environment without
a JS engine. Keeping the tests in-tree during the `web-sys` removal refactor
would have inflated the apparent surface area of the refactor by ~6,000 lines
and prevented us from ever making the crate compile against Paws'
`rust-wasm-binding`.

## How the archive is organized

The directory tree mirrors `packages/yew/src/` (and `packages/yew/tests/`)
so each archived file lives where the original file used to live:

```
tests-archive/
├── README.md                            # (this file)
├── dom_bundle/
│   ├── bcomp.rs                         # pre-deletion full file
│   ├── blist.rs
│   ├── bnode.rs
│   ├── bportal.rs
│   ├── btext.rs
│   ├── position.rs
│   ├── utils.rs
│   └── btag/
│       ├── mod.rs
│       ├── listeners.rs
│       └── attributes.rs
├── html/
│   └── component/
│       └── lifecycle.rs
├── virtual_dom/
│   └── key.rs
├── src_tests/                           # former packages/yew/src/tests/
│   ├── mod.rs
│   └── layout_tests.rs                  # TestLayout helper
└── integration/                         # former packages/yew/tests/
    ├── common/mod.rs
    ├── layout.rs
    ├── mod.rs
    ├── suspense.rs
    ├── use_callback.rs
    ├── use_context.rs
    ├── use_effect.rs
    ├── use_memo.rs
    ├── use_reducer.rs
    ├── use_ref.rs
    └── use_state.rs
```

Each file in `dom_bundle/`, `html/`, and `virtual_dom/` is the **full
pre-deletion contents** (production code + tests). The production code is
duplicated with what's live in `src/` and is intentionally inert — when
porting, reach for the `#[cfg(test)]` modules and the inline test helpers
(e.g. `DomSlot::get()` in `position.rs`, `BTag::reference()` / `children()`
/ `tag()` in `btag/mod.rs`).

## How to port a test

The tests break down into three buckets:

### Bucket 1: Pure Rust (no DOM, no browser runtime)

Tests that only exercise `html!` macro parsing and type checking. Example
from `dom_bundle/bcomp.rs`:

```rust
#[test]
fn set_properties_to_component() {
    html! { <Comp /> };
    html! { <Comp field_1=1 /> };
    html! { <Comp field_2=2 /> };
}
```

**How to port:** strip any `#[cfg(all(target_arch = "wasm32", ...))]` and
`wasm_bindgen_test_configure!` invocations, drop the `wasm_bindgen_test`
import, and move the test into a plain `#[cfg(test)] mod tests_without_browser`
block inside the corresponding `src/` file. These can land immediately — they
don't depend on Phase 2 (the `web-sys` removal). Today's `btag/mod.rs`
already has a `tests_without_browser` module of this shape.

### Bucket 2: Reconciliation / layout tests

Tests that call `Reconcilable::attach()` on a real parent `Element` and then
inspect the resulting DOM structure (via `inner_html()`,
`get_element_by_id()`, etc.). Example: `update_loop` in `bcomp.rs`, or any
`layout_tests::diff` fixture built from `TestLayout`.

**How to port:** wait until yew's production code is compiling against
`rust-wasm-binding` (Phase 2). Then:

1. Create a new WASM fixture crate under `Paws/examples/` (mirroring the
   pattern used by `example-namespace`, etc.).
2. Move the test body into a `#[no_mangle] pub extern "C" fn run()` entry
   point that builds the yew virtual tree and calls `scheduler::start_now()`.
3. Write a host-side test in `wasmtime-engine/tests/` that loads the fixture,
   runs it, then walks `RuntimeState::doc` to assert the expected DOM
   structure. Use `get_attribute` / `get_first_child` / `get_node_type` host
   calls or access `state.doc` directly from the test.
4. Replace `inner_html()` comparisons with tree-walking assertions against
   node IDs / tag names / text content.

### Bucket 3: Event / interaction tests

Tests that dispatch synthetic events and verify handlers fired (counters,
text content updates, etc.). Example: everything in
`dom_bundle/btag/listeners.rs`.

**How to port:** same WASM fixture pattern as Bucket 2, plus:

1. Use `rust_wasm_binding::dispatch_event(target_id, "click", true, true, false)`
   from the host side to fire events.
2. Yew registers listeners via `rust_wasm_binding::add_event_listener`;
   callbacks are invoked through the guest-exported `__paws_invoke_listener`
   entry point that the host calls during W3C three-phase dispatch
   (implemented in Paws PR #57).
3. Assert against component state via `NodeRef` reads or by inspecting the
   resulting DOM tree on `RuntimeState::doc`.

## Tracking

A full port takes this whole directory to zero. Each ported file can be
deleted from the archive as a self-contained follow-up commit.
