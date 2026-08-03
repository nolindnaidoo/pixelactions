//! Where the run record goes, and when.
//!
//! The record itself is `pixelactions_core::audit` — pure, no clock, no
//! file. This is the half that needs an OS: a path, a wall clock, and an
//! append.
//!
//! **Failing to write never fails a run.** Same rule the Wayland token
//! follows: a run that is otherwise granted and working must not die
//! because a log file could not be opened. Every function here swallows
//! its errors deliberately, and that is the one place in this codebase
//! where that is correct.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use pixelactions_core::audit::Event;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Where the log lives.
///
/// `XDG_STATE_HOME` for the same reason the Wayland restore token uses
/// it: this is neither configuration a human edits nor a cache that can
/// be cleared without consequence.
///
/// **One file, appended to, never pruned.** A run writes on the order of
/// a kilobyte, so a thousand runs is a megabyte — and deleting someone's
/// own record of what their machine did, to save that, is not a trade
/// this tool gets to make. Documented so it can be cleared by hand.
#[must_use]
pub fn log_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(base.join("pixelactions").join("audit.ndjson"))
}

/// Now, as RFC 3339. Falls back to the epoch rather than refusing to log:
/// a record with a wrong timestamp is worth more than no record.
#[must_use]
pub fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// Append one event, quietly.
pub fn append(event: &Event) {
    let Some(path) = log_path() else { return };
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let _ = file.write_all(event.line().as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_sits_under_the_state_directory() {
        // SAFETY: single-threaded test, restored below.
        unsafe { std::env::set_var("XDG_STATE_HOME", "/tmp/pxa-audit-test") };
        let path = log_path().expect("a path");
        assert_eq!(
            path,
            PathBuf::from("/tmp/pxa-audit-test/pixelactions/audit.ndjson")
        );
        // SAFETY: as above.
        unsafe { std::env::remove_var("XDG_STATE_HOME") };
    }

    #[test]
    fn now_is_rfc_3339_and_not_the_fallback() {
        let stamp = now();
        assert!(stamp.ends_with('Z'), "{stamp}");
        assert!(stamp.len() >= 20, "{stamp}");
        assert_ne!(stamp, "1970-01-01T00:00:00Z");
    }
}
