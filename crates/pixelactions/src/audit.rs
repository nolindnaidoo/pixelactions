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

use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use pixelactions_core::audit::Event;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Where the log lives, given what the environment says.
///
/// Pure, and takes `windows` explicitly, so all three platforms'
/// resolution is testable from any one of them — which matters here
/// because the bug this replaced was invisible on the platform it broke.
///
/// `XDG_STATE_HOME` wins everywhere when it is set: someone who exports
/// it has said where they want state, and Git Bash on Windows is a real
/// case. Otherwise Windows uses `%LOCALAPPDATA%` — the non-roaming
/// per-user store, which is what a log is — and everything else uses the
/// XDG default.
fn state_dir(
    xdg_state: Option<&OsStr>,
    local_appdata: Option<&OsStr>,
    user_profile: Option<&OsStr>,
    home: Option<&OsStr>,
    windows: bool,
) -> Option<PathBuf> {
    if let Some(xdg) = xdg_state {
        return Some(PathBuf::from(xdg));
    }
    if windows {
        if let Some(local) = local_appdata {
            return Some(PathBuf::from(local));
        }
        return user_profile.map(|p| PathBuf::from(p).join("AppData").join("Local"));
    }
    home.map(|home| PathBuf::from(home).join(".local").join("state"))
}

/// Where the log lives.
///
/// Neither configuration a human edits nor a cache that can be cleared
/// without consequence — the same reasoning the Wayland restore token
/// uses, and the same directory on the platforms that have one.
///
/// **One file, appended to, never pruned.** A run writes on the order of
/// a kilobyte, so a thousand runs is a megabyte — and deleting someone's
/// own record of what their machine did, to save that, is not a trade
/// this tool gets to make. Documented so it can be cleared by hand.
#[must_use]
pub fn log_path() -> Option<PathBuf> {
    let dir = state_dir(
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        std::env::var_os("LOCALAPPDATA").as_deref(),
        std::env::var_os("USERPROFILE").as_deref(),
        std::env::var_os("HOME").as_deref(),
        cfg!(target_os = "windows"),
    )?;
    Some(dir.join("pixelactions").join("audit.ndjson"))
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

    fn dir(
        xdg: Option<&str>,
        local: Option<&str>,
        profile: Option<&str>,
        home: Option<&str>,
        windows: bool,
    ) -> Option<String> {
        state_dir(
            xdg.map(OsStr::new),
            local.map(OsStr::new),
            profile.map(OsStr::new),
            home.map(OsStr::new),
            windows,
        )
        .map(|p| p.display().to_string())
    }

    /// The bug this replaced: Windows sets neither `XDG_STATE_HOME` nor
    /// `HOME` under cmd or PowerShell, so the log resolved to nothing and
    /// every run silently recorded nothing at all.
    #[test]
    fn windows_resolves_without_xdg_or_home() {
        assert_eq!(
            dir(None, Some(r"C:\Users\me\AppData\Local"), None, None, true),
            Some(r"C:\Users\me\AppData\Local".to_string())
        );
    }

    /// Without LOCALAPPDATA either, the profile still gets there.
    ///
    /// Checked by components rather than by rendered string: `join` uses
    /// the *host* separator, so a string comparison would only pass on
    /// Windows — and Windows is the platform this test exists to cover
    /// from somewhere else.
    #[test]
    fn windows_falls_back_to_the_user_profile() {
        let resolved =
            state_dir(None, None, Some(OsStr::new(r"C:\Users\me")), None, true).expect("a path");
        let tail: Vec<String> = resolved
            .components()
            .rev()
            .take(2)
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        assert_eq!(tail, ["Local", "AppData"], "{}", resolved.display());
        assert!(
            resolved.display().to_string().starts_with(r"C:\Users\me"),
            "{}",
            resolved.display()
        );
    }

    /// By components, for the same reason the Windows case is: `join` uses
    /// the host separator, so a rendered-string comparison passes only on
    /// the platform that wrote it. This one failed on Windows CI.
    #[test]
    fn unix_uses_the_xdg_default() {
        let resolved =
            state_dir(None, None, None, Some(OsStr::new("/home/me")), false).expect("a path");
        let tail: Vec<String> = resolved
            .components()
            .rev()
            .take(2)
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        assert_eq!(tail, ["state", ".local"], "{}", resolved.display());
    }

    /// An explicit export wins everywhere — including Windows, where Git
    /// Bash is a real case.
    #[test]
    fn an_explicit_xdg_state_home_wins_on_every_platform() {
        for windows in [true, false] {
            assert_eq!(
                dir(
                    Some("/somewhere"),
                    Some("C:/local"),
                    None,
                    Some("/home/me"),
                    windows
                ),
                Some("/somewhere".to_string()),
                "windows={windows}"
            );
        }
    }

    /// Nothing to go on is still None — the caller writes nothing rather
    /// than guessing at a path.
    #[test]
    fn nothing_to_go_on_resolves_to_nothing() {
        assert_eq!(dir(None, None, None, None, true), None);
        assert_eq!(dir(None, None, None, None, false), None);
    }

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
