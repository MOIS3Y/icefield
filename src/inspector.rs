//! Drift detection and state inspection.
//!
//! This module provides functionality to compare the desired state recorded
//! in `state.json` with the actual state of the filesystem, reporting any
//! discrepancies (drift) to the user.

use crate::state::State;
use crate::utils::hash_file;
use anyhow::Result;
use console::style;
use std::fs;
use std::path::Path;

/// Represents the possible drift states of a managed file.
enum Status {
    /// The file exists and matches the desired state perfectly.
    Ok,
    /// The file exists but its content or symlink target has diverged.
    Modified,
    /// The file was expected but could not be found on the filesystem.
    Missing,
}

/// Orchestrator entry point for the info command.
///
/// Inspects the managed files and prints their drift status.
pub fn inspect(state_path: &Path) -> Result<()> {
    let state = State::load(&state_path.to_path_buf())?;

    println!(
        "{} Icefield: System Configuration Status\nFound {} managed files:\n",
        style("❄").blue(),
        style(state.managed_files.len()).bold()
    );

    let mut ok_count = 0;
    let mut modified_count = 0;
    let mut missing_count = 0;

    let total_files = state.managed_files.len();

    for (path, state_info) in &state.managed_files {
        let expected_value = &state_info.hash;
        let derivation_name = &state_info.name;

        let path_exists = path.exists() || fs::symlink_metadata(path).is_ok();

        if !path_exists {
            print_missing(path, derivation_name);
            missing_count += 1;
            continue;
        }

        let status = if expected_value.starts_with("symlink:") {
            inspect_symlink(path, expected_value, derivation_name)
        } else {
            inspect_regular_file(path, expected_value, derivation_name)
        };

        match status {
            Status::Ok => ok_count += 1,
            Status::Modified => modified_count += 1,
            Status::Missing => missing_count += 1,
        }
    }

    if total_files > 0 {
        print_summary(ok_count, modified_count, missing_count);
    }

    Ok(())
}

/// Helper to print a missing file status.
fn print_missing(path: &Path, name: &str) {
    println!(
        "  {} {} {} {}",
        style("[ MISSING ]").red().bold(),
        style(path.display()).cyan(),
        style(format!("[{}]", name)).dim(),
        style("(file not found on disk)").dim()
    );
}

/// Helper to print the final summary.
fn print_summary(
    ok_count: usize,
    modified_count: usize,
    missing_count: usize,
) {
    println!(
        "\nSummary: {} OK, {} MODIFIED, {} MISSING",
        style(ok_count).green(),
        style(modified_count).yellow(),
        style(missing_count).red()
    );
}

/// Inspects a symbolic link and compares it against its expected target.
///
/// This function verifies that the path exists, is actually a symlink,
/// and points to the exact absolute path that was recorded during the
/// last successful application.
fn inspect_symlink(path: &Path, expected_value: &str, name: &str) -> Status {
    let expected_target = expected_value.strip_prefix("symlink:").unwrap();

    // Canonicalize expected target to match what apply_symlink does
    let expected_target_path = Path::new(expected_target);
    let canonical_expected = fs::canonicalize(expected_target_path)
        .unwrap_or_else(|_| expected_target_path.to_path_buf());
    let canonical_expected_str = canonical_expected.to_string_lossy();

    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => match fs::read_link(path)
        {
            Ok(actual_target) => {
                let actual_target_str = actual_target.to_string_lossy();
                if actual_target_str == canonical_expected_str {
                    println!(
                        "  {} {} {} {} {}",
                        style("[   OK    ]").green().bold(),
                        style(path.display()).cyan(),
                        style("->").dim(),
                        style(expected_target).dim(),
                        style(format!("[{}]", name)).dim()
                    );
                    Status::Ok
                } else {
                    println!(
                        "  {} {} {} {}",
                        style("[ MODIFIED]").yellow().bold(),
                        style(path.display()).cyan(),
                        style(format!("[{}]", name)).dim(),
                        style(format!(
                            "(symlink drift: points to {})",
                            actual_target_str
                        ))
                        .dim()
                    );
                    Status::Modified
                }
            }
            Err(_) => {
                println!(
                    "  {} {} {} {}",
                    style("[ MODIFIED]").yellow().bold(),
                    style(path.display()).cyan(),
                    style(format!("[{}]", name)).dim(),
                    style("(failed to read symlink target)").dim()
                );
                Status::Modified
            }
        },
        _ => {
            println!(
                "  {} {} {} {}",
                style("[ MODIFIED]").yellow().bold(),
                style(path.display()).cyan(),
                style(format!("[{}]", name)).dim(),
                style("(expected symlink, found regular file/dir)").dim()
            );
            Status::Modified
        }
    }
}

/// Inspects a regular file by hashing its content and comparing it.
///
/// This function calculates the SHA-256 hash of the file currently on disk
/// and checks if it matches the hash recorded in the state database. It also
/// detects type mismatches (e.g., if a symlink replaced a regular file).
fn inspect_regular_file(
    path: &Path,
    expected_value: &str,
    name: &str,
) -> Status {
    if fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        println!(
            "  {} {} {} {}",
            style("[ MODIFIED]").yellow().bold(),
            style(path.display()).cyan(),
            style(format!("[{}]", name)).dim(),
            style("(expected regular file, found symlink)").dim()
        );
        return Status::Modified;
    }

    match hash_file(path) {
        Ok(actual_hash) => {
            if actual_hash == *expected_value {
                println!(
                    "  {} {} {} {}",
                    style("[   OK    ]").green().bold(),
                    style(path.display()).cyan(),
                    style(format!("[{}]", name)).dim(),
                    style(format!("[{}]", &expected_value[..8])).dim()
                );
                Status::Ok
            } else {
                println!(
                    "  {} {} {} {}",
                    style("[ MODIFIED]").yellow().bold(),
                    style(path.display()).cyan(),
                    style(format!("[{}]", name)).dim(),
                    style("(content drift detected)").dim()
                );
                Status::Modified
            }
        }
        Err(_) => {
            println!(
                "  {} {} {} {}",
                style("[  ERROR  ]").red().bold(),
                style(path.display()).cyan(),
                style(format!("[{}]", name)).dim(),
                style("(failed to read file for hashing)").dim()
            );
            // Treating read errors as missing/broken
            Status::Missing
        }
    }
}
