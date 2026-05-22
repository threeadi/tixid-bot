use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

const LOG_FILE: &str = "tixid-bot.log";

/// Initialises the logging system.
///
/// Returns a `WorkerGuard` that **must be held alive until the end of `main()`**.
/// Dropping it signals the background writer thread to flush all buffered
/// entries and exit cleanly — so no log entries are lost on shutdown.
///
/// Architecture:
///   caller code
///     └─ tracing macro  (just formats a string + sends to channel, ~1µs)
///         └─ mpsc channel
///             └─ background thread  (reads channel, writes to disk)
///
/// The hot path (your bot code) never blocks on disk I/O.
///
/// Log level is controlled by RUST_LOG env var (default: debug for this crate, info for deps).
pub fn init() -> WorkerGuard {
    let appender = tracing_appender::rolling::never(".", LOG_FILE);
    let (non_blocking_file, guard) = tracing_appender::non_blocking(appender);

    // Default: tixid_bot=debug so request/response bodies are captured;
    // deps stay at info to avoid noise. Override via RUST_LOG env var.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("tixid_bot=debug,info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(non_blocking_file)
                .with_ansi(false)
                .with_target(false),
        )
        .init();

    guard
}
