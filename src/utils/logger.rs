//! Logging initialization and configuration.
//!
//! This module provides functionality to initialize logging for the application,
//! allowing for debug and error tracking during development and production.
//!
//! Logs are written to files in the `logs/` directory to avoid interfering with the TUI.
//! Log files are automatically rotated daily.
//!
//! # Configuration
//!
//! The log level can be controlled via the `RUST_LOG` environment variable:
//! - `RUST_LOG=debug` - Show debug and higher level logs
//! - `RUST_LOG=info` - Show info and higher level logs (default)
//! - `RUST_LOG=warn` - Show warnings and errors only
//! - `RUST_LOG=error` - Show errors only

use std::fs;
use std::path::PathBuf;
use chrono::Local;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize the logging system.
///
/// This sets up file-based logging with a unique file per run to avoid interfering
/// with the TUI interface. Logs are written to the `logs/` directory in the
/// executable's directory.
///
/// Each run creates a new log file with a timestamp, e.g.:
/// `logs/rusty-term.2024-12-06-14-30-25.log`
///
/// The log level is controlled by the `RUST_LOG` environment variable,
/// defaulting to `info` if not set.
pub fn init_logging() {
    // Get the directory where the executable is located
    let log_dir = match std::env::current_exe() {
        Ok(exe_path) => {
            // Get the parent directory of the executable
            exe_path.parent()
                .map(|p| p.join("logs"))
                .unwrap_or_else(|| PathBuf::from("logs"))
        }
        Err(_) => {
            // Fallback to current directory if we can't get exe path
            PathBuf::from("logs")
        }
    };

    // Ensure the logs directory exists
    if let Err(e) = fs::create_dir_all(&log_dir) {
        eprintln!("Warning: Failed to create logs directory: {}", e);
        return;
    }

    // Generate a unique filename with timestamp for this run
    // Format: rusty-term.2024-12-06-14-30-25.log
    let timestamp = Local::now().format("%Y-%m-%d-%H-%M-%S");
    let log_filename = format!("rusty-term.{}.log", timestamp);
    let log_path = log_dir.join(&log_filename);

    // Create the log file
    let log_file = match fs::File::create(&log_path) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Warning: Failed to create log file: {}", e);
            return;
        }
    };

    // Use non-blocking writer to avoid blocking the TUI
    let (non_blocking, _guard) = tracing_appender::non_blocking(log_file);

    // Create the file layer with formatting
    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)  // No ANSI colors in log files
        .with_target(true)  // Include module path
        .with_thread_ids(true)  // Include thread IDs for debugging
        .with_line_number(true);  // Include line numbers

    // Configure environment filter
    // Default to "info" level if RUST_LOG is not set
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // Build and initialize the subscriber
    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .init();

    // We intentionally leak the _guard to keep the non-blocking writer alive
    // for the entire program lifetime. This is acceptable for a main application.
    std::mem::forget(_guard);

    tracing::info!("Logging initialized - writing to {}", log_path.display());
}
