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

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;

#[cfg(target_arch = "wasm32")]
use futures::task::LocalSpawnExt;

#[cfg(target_arch = "wasm32")]
struct LocalRuntime {
    pool: RefCell<futures::executor::LocalPool>,
    spawner: futures::executor::LocalSpawner,
}

#[cfg(target_arch = "wasm32")]
impl LocalRuntime {
    fn new() -> Self {
        let pool = futures::executor::LocalPool::new();
        let spawner = pool.spawner();
        Self {
            pool: RefCell::new(pool),
            spawner,
        }
    }
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static LOCAL_RUNTIME: LocalRuntime = LocalRuntime::new();
}

#[cfg(target_arch = "wasm32")]
fn run_until_stalled() {
    LOCAL_RUNTIME.with(|runtime| {
        if let Ok(mut pool) = runtime.pool.try_borrow_mut() {
            pool.run_until_stalled();
        }
    });
}

/// Run a non-Send future on the current thread.
///
/// This intentionally does not depend on any browser-oriented async backend.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_local<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    futures::executor::block_on(future);
}

/// Run a non-Send future on the current thread.
///
/// On WAMR/WASI the future is polled until it yields. Host callbacks wake the
/// same local pool instead of blocking the wasm entry call.
#[cfg(target_arch = "wasm32")]
pub fn spawn_local<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    LOCAL_RUNTIME.with(|runtime| {
        runtime
            .spawner
            .spawn_local(future)
            .expect("failed to spawn local future");
    });
    run_until_stalled();
}

/// Timer helpers.
pub mod time {
    use std::time::Duration;

    /// Sleep for the requested duration.
    ///
    /// On `wasm32-wasip1` this uses the WASI-backed standard library sleep,
    /// which is provided by the WAMR runtime.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn sleep(duration: Duration) {
        std::thread::sleep(duration);
    }

    /// Sleep for the requested duration.
    ///
    /// On WAMR/WASI this yields to the host timer import and wakes the local
    /// executor from the timer callback.
    #[cfg(target_arch = "wasm32")]
    pub async fn sleep(duration: Duration) {
        Sleep::new(duration).await;
    }

    #[cfg(target_arch = "wasm32")]
    mod wasm_timer {
        use std::cell::RefCell;
        use std::future::Future;
        use std::pin::Pin;
        use std::rc::Rc;
        use std::task::{Context, Poll, Waker};
        use std::time::Duration;

        use rust_wasm_binding::TimerId;

        use crate::platform::run_until_stalled;

        struct SleepState {
            fired: bool,
            timer_id: Option<TimerId>,
            waker: Option<Waker>,
        }

        pub struct Sleep {
            state: Rc<RefCell<SleepState>>,
        }

        impl Sleep {
            pub fn new(duration: Duration) -> Self {
                let state = Rc::new(RefCell::new(SleepState {
                    fired: false,
                    timer_id: None,
                    waker: None,
                }));
                let callback_state = state.clone();
                let delay_ms = duration.as_millis().min(i64::MAX as u128) as i64;
                let timer_id = rust_wasm_binding::set_timeout(
                    move || {
                        let waker = {
                            let mut state = callback_state.borrow_mut();
                            state.fired = true;
                            state.timer_id = None;
                            state.waker.take()
                        };
                        if let Some(waker) = waker {
                            waker.wake();
                        }
                        run_until_stalled();
                    },
                    delay_ms,
                );
                state.borrow_mut().timer_id = Some(timer_id);
                Self { state }
            }
        }

        impl Future for Sleep {
            type Output = ();

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let mut state = self.state.borrow_mut();
                if state.fired {
                    Poll::Ready(())
                } else {
                    state.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        }

        impl Drop for Sleep {
            fn drop(&mut self) {
                let timer_id = {
                    let mut state = self.state.borrow_mut();
                    if state.fired {
                        None
                    } else {
                        state.timer_id.take()
                    }
                };
                if let Some(timer_id) = timer_id {
                    rust_wasm_binding::clear_timeout(timer_id);
                }
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    use wasm_timer::Sleep;
}
