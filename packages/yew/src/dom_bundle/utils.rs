/// Log an operation during tests for debugging purposes.
/// Currently a no-op; can be wired to a real logger in the future.
macro_rules! test_log {
    ($fmt:literal, $($arg:expr),* $(,)?) => {
        // Only type-check the format expression, do not run any side effects
        let _ = || { std::format_args!(concat!("\t  ", $fmt), $($arg),*); };
    };
}

pub(super) use test_log;
