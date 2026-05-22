use tracing::Level;
use tracing_subscriber::fmt;

/// Initializes the global tracing subscriber with the specified verbosity level.
///
/// - 0: INFO
/// - 1: DEBUG
/// - 2+: TRACE
pub fn setup(verbosity: u8) {
    let level = match verbosity {
        0 => Level::INFO,
        1 => Level::DEBUG,
        _ => Level::TRACE,
    };

    fmt().with_max_level(level).with_target(false).init();
}
