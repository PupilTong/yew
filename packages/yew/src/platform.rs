//! Yew's compatibility layer for the WAMR/WASI runtime.
//!
//! # Rationale
//!
//! When designing components and libraries for WAMR-backed WebAssembly and native test targets,
//! developers usually face challenges that require applying multiple feature flags throughout
//! their application:
//!
//! 1. Select I/O and timers that works with the target runtime.
//! 2. Native runtimes usually require `Send` futures and WebAssembly types are usually `!Send`.
//!
//! # Implementation
//!
//! To alleviate these issues, Yew keeps a small local API surface for spawning
//! `?Send` (`Send` or `!Send`) futures and sleeping.
//!
//! Yew platform provides the following components:
//!
//! 1. A task entry point that is capable of running non-Send tasks.
//! 2. A timer helper that is compatible with the selected runtime.
//!
//! # Runtime Backend
//!
//! The Yew runtime is implemented with different backends depending on the target platform:
//!
//! - WAMR and WASI for `wasm32-wasip1`
//! - standard Rust futures utilities for native test targets

use std::future::Future;

/// Run a non-Send future on the current thread.
///
/// This intentionally does not depend on any browser-oriented async backend.
pub fn spawn_local<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    futures::executor::block_on(future);
}

/// Timer helpers.
pub mod time {
    use std::time::Duration;

    /// Sleep for the requested duration.
    ///
    /// On `wasm32-wasip1` this uses the WASI-backed standard library sleep,
    /// which is provided by the WAMR runtime.
    pub async fn sleep(duration: Duration) {
        std::thread::sleep(duration);
    }
}
