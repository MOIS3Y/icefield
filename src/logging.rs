//! Audit logging and diagnostics.
//!
//! This module implements the file-based logging system that records detailed
//! execution information for auditing and troubleshooting, while keeping the
//! main console output clean and professional.

use std::path::Path;
use tracing::Level;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::time::LocalTime;
use tracing_subscriber::prelude::*;

/// Initializes the global tracing subscriber with file-based logging.
///
/// - All logs based on `verbosity` are written to `icefield.log` in `cache_dir`.
/// - Console output is disabled to allow for a custom styled UI.
/// - Uses local time and includes file/line information for better auditing.
///
/// Returns a `WorkerGuard` that must be held by `main` to ensure logs are flushed.
pub fn setup(
    verbosity: u8,
    cache_dir: &Path,
) -> tracing_appender::non_blocking::WorkerGuard {
    let level = match verbosity {
        0 => Level::INFO,
        1 => Level::DEBUG,
        _ => Level::TRACE,
    };

    // Create a rolling file appender
    let file_appender =
        tracing_appender::rolling::never(cache_dir, crate::paths::LOG_FILE);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // File layer for technical logs
    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .with_timer(LocalTime::rfc_3339())
        .with_filter(tracing_subscriber::filter::LevelFilter::from_level(
            level,
        ));

    // Register the subscriber
    tracing_subscriber::registry().with(file_layer).init();

    guard
}
