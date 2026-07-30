//! Which display server this session is actually running.
//!
//! On macOS and Windows the windowing system is a compile-time fact. On
//! Linux it is not: the same binary on the same machine faces X11 or
//! Wayland depending on which session the user logged into, and the two
//! need different injection paths entirely.
//!
//! Getting this wrong is not a crash, it is worse. Injecting through
//! `XWayland` on a Wayland session reaches X clients only, so the pointer
//! travels over native windows that never receive the events — a run that
//! clicks through *some* windows and not others, reporting success either
//! way. So the answer is decided once, here, from the environment, and a
//! session we cannot name is refused rather than assumed.
//!
//! The environment is passed in rather than read here: this crate is
//! platform-free, and a decision that takes its inputs as arguments is a
//! decision that can be tested against every session shape without
//! needing that session.

use serde::Serialize;

/// The display server a Linux session is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Server {
    Wayland,
    X11,
    /// Nothing in the environment identifies a session — the usual case
    /// being a plain TTY, a container, or a cron job with no desktop.
    Unknown,
}

impl Server {
    pub fn name(self) -> &'static str {
        match self {
            Self::Wayland => "wayland",
            Self::X11 => "x11",
            Self::Unknown => "unknown",
        }
    }
}

/// Decide from the three variables that describe a Linux session.
///
/// `XDG_SESSION_TYPE` is the authority when it says something meaningful,
/// because it is what the login session itself declares. The socket
/// variables are the fallback, and `WAYLAND_DISPLAY` is checked first on
/// purpose: a Wayland session almost always *also* sets `DISPLAY`, for
/// `XWayland`. Trusting `DISPLAY` first would misread nearly every modern
/// desktop as X11 — which is exactly the half-working failure above.
pub fn detect(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    x_display: Option<&str>,
) -> Server {
    let declared = session_type.map(str::trim).unwrap_or_default();
    match declared.to_ascii_lowercase().as_str() {
        "wayland" => return Server::Wayland,
        "x11" => return Server::X11,
        _ => {}
    }
    if present(wayland_display) {
        return Server::Wayland;
    }
    if present(x_display) {
        return Server::X11;
    }
    Server::Unknown
}

/// A variable set to the empty string is not set, as far as a socket name
/// is concerned. Shells produce this often enough to matter.
fn present(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declared_session_type_wins() {
        assert_eq!(detect(Some("wayland"), None, None), Server::Wayland);
        assert_eq!(detect(Some("x11"), None, None), Server::X11);
        // Even when the other variable disagrees, which is the normal
        // case on a Wayland desktop running XWayland.
        assert_eq!(
            detect(Some("wayland"), Some("wayland-0"), Some(":0")),
            Server::Wayland
        );
        assert_eq!(detect(Some("x11"), None, Some(":0")), Server::X11);
    }

    #[test]
    fn case_and_whitespace_in_the_declaration_are_tolerated() {
        assert_eq!(detect(Some("Wayland"), None, None), Server::Wayland);
        assert_eq!(detect(Some(" X11 "), None, None), Server::X11);
    }

    /// The variable exists but says something else — `tty`, on a console
    /// login. The sockets decide from there.
    #[test]
    fn an_unhelpful_declaration_falls_through_to_the_sockets() {
        assert_eq!(
            detect(Some("tty"), Some("wayland-0"), None),
            Server::Wayland
        );
        assert_eq!(detect(Some("tty"), None, Some(":0")), Server::X11);
        assert_eq!(detect(Some("tty"), None, None), Server::Unknown);
    }

    /// The case this ordering exists for: both sockets set, which is what
    /// every GNOME and KDE Wayland session looks like.
    #[test]
    fn wayland_wins_over_a_leftover_x_display() {
        assert_eq!(detect(None, Some("wayland-0"), Some(":0")), Server::Wayland);
    }

    #[test]
    fn nothing_set_is_unknown_rather_than_a_guess() {
        assert_eq!(detect(None, None, None), Server::Unknown);
    }

    #[test]
    fn an_empty_variable_is_not_a_session() {
        assert_eq!(detect(Some(""), Some(""), Some("")), Server::Unknown);
        assert_eq!(detect(None, Some("  "), Some(":0")), Server::X11);
    }

    #[test]
    fn every_server_has_a_name() {
        assert_eq!(Server::Wayland.name(), "wayland");
        assert_eq!(Server::X11.name(), "x11");
        assert_eq!(Server::Unknown.name(), "unknown");
    }
}
