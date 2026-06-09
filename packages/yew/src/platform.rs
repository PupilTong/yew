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
//! 1. A cooperative single-threaded executor for non-Send tasks, advanced by
//!    the scheduler rather than a background event loop.
//! 2. A timer helper that is compatible with the selected runtime.
//!
//! # Runtime Backend
//!
//! The Yew runtime is implemented with different backends depending on the target platform:
//!
//! - WAMR and WASI for `wasm32-wasip1`
//! - standard Rust futures utilities for native test targets

use std::cell::RefCell;
use std::future::Future;

use futures::executor::{LocalPool, LocalSpawner};
use futures::task::LocalSpawnExt;

thread_local! {
    /// Single-threaded executor that owns every future handed to
    /// [`spawn_local`]. It is advanced cooperatively by the scheduler via
    /// [`drive_spawned_tasks`], not by a background event loop.
    static LOCAL_POOL: RefCell<LocalPool> = RefCell::new(LocalPool::new());
    /// Spawner handle for `LOCAL_POOL`. Kept separate so spawning a task never
    /// borrows the pool itself, which may be mid-run inside
    /// [`drive_spawned_tasks`].
    static SPAWNER: LocalSpawner = LOCAL_POOL.with(|pool| pool.borrow().spawner());
}

/// Spawn a non-Send future onto the current thread's executor.
///
/// The future is queued and control returns to the caller immediately — it is
/// **not** run to completion here. The scheduler drives queued futures forward
/// after it drains its render/lifecycle work (see [`drive_spawned_tasks`]), so
/// `spawn_local` keeps the fire-and-forget semantics yew relies on for
/// `Scope::send_future`, streams, and suspense — without blocking the caller or
/// depending on a browser / `tokio` backend.
pub fn spawn_local<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    SPAWNER.with(|spawner| {
        // `spawn_local` only errors once the pool has been dropped, which never
        // happens for a thread-local that lives as long as the thread.
        let _ = spawner.spawn_local(future);
    });
}

/// Advance every spawned future as far as it can progress without blocking.
///
/// Called by the scheduler once its runnable queue is empty. Futures that
/// resolve here may resume suspensions or send component messages, which the
/// scheduler then picks up on its next pass. Futures still awaiting an external
/// wake-up (e.g. a host timer or event) stay parked until the host re-enters
/// the guest.
pub(crate) fn drive_spawned_tasks() {
    LOCAL_POOL.with(|pool| {
        // Skip if a drive is already in progress on this thread (re-entrancy
        // guard); the outer drive picks up any newly woken tasks.
        if let Ok(mut pool) = pool.try_borrow_mut() {
            pool.run_until_stalled();
        }
    });
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
