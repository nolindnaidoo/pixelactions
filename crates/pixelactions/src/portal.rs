//! The consent half of Wayland input: xdg-desktop-portal `RemoteDesktop`,
//! linked to a `ScreenCast` session, ending in an EIS socket.
//!
//! Wayland forbids cross-client input injection by design, so unlike every
//! other platform there is no call to make — there is a *negotiation*, and
//! the user is part of it. This module owns that negotiation and nothing
//! else: it returns a connected socket and the geometry that came with it,
//! and knows nothing about clicks.
//!
//! Three decisions worth stating, because each has a cheaper wrong answer:
//!
//! - **Blocking D-Bus, no async runtime.** zbus's blocking API is used
//!   directly rather than `ashpd`, which is async-only. This tool runs,
//!   acts, and exits; adding an executor to make six method calls would
//!   buy nothing and cost a rule (see AGENTS.md). The price is the
//!   Request/Response dance below, written out once.
//! - **A linked `ScreenCast` session is mandatory, not optional.** Absolute
//!   pointer placement is only meaningful inside a region the compositor
//!   grants, and it derives those regions from the streams the user
//!   consented to share. No stream, no absolute placement — so `plan`
//!   would work and `run` would silently become relative. We ask for the
//!   stream up front.
//! - **The token is persisted so the dialog happens at setup time.** A
//!   consent prompt appearing in the middle of an unattended run is worse
//!   than a refusal, because a flow half-executes while a human is asked
//!   a question they are not there to answer.

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const DEST: &str = "org.freedesktop.portal.Desktop";
const OBJECT: &str = "/org/freedesktop/portal/desktop";
const REMOTE_DESKTOP: &str = "org.freedesktop.portal.RemoteDesktop";
const SCREEN_CAST: &str = "org.freedesktop.portal.ScreenCast";

/// KEYBOARD | POINTER. Touchscreen is offered by the portal and unused: a
/// flow describes clicks and keystrokes, and asking for a capability we
/// never exercise widens the grant for nothing.
const DEVICES_WANTED: u32 = 1 | 2;
/// MONITOR. Window sources would tie a run to one window's lifetime.
const SOURCE_MONITOR: u32 = 1;
/// `persist_mode` 2 — until the user revokes it.
const PERSIST_UNTIL_REVOKED: u32 = 2;
/// The `RemoteDesktop` interface version that added `ConnectToEIS`,
/// `restore_token` and `persist_mode`. Below this there is no EIS socket
/// to connect to and no way to avoid prompting on every run.
const MIN_REMOTE_DESKTOP_VERSION: u32 = 2;

type Results = HashMap<String, OwnedValue>;

/// What the portal handed over: a live EIS socket plus the session it
/// belongs to.
///
/// The D-Bus connection is kept inside deliberately. The portal ties the
/// session's lifetime to the connection that created it, so dropping it
/// early revokes the grant and the socket goes dead mid-run.
pub struct Grant {
    eis: Option<UnixStream>,
    _session: OwnedObjectPath,
    _connection: Connection,
}

impl Grant {
    /// Hand over the EIS socket, once.
    ///
    /// Moved out rather than cloned: two descriptors on one EIS
    /// connection would keep it alive past the sender that owns it, and
    /// there is no reason for a second one to exist. The `Grant` itself
    /// must still be held — it is what keeps the session granted.
    pub fn take_socket(&mut self) -> Result<UnixStream> {
        self.eis
            .take()
            .ok_or_else(|| anyhow!("the EIS socket has already been taken"))
    }
}

/// What the portal offers on this machine, for `doctor` to report without
/// asking for anything or raising a dialog.
#[derive(Debug)]
pub struct Capabilities {
    pub remote_desktop_version: u32,
    pub screen_cast_version: u32,
    pub device_types: u32,
    pub cursor_modes: u32,
    pub have_stored_token: bool,
}

impl Capabilities {
    /// Whether rung A — portal `RemoteDesktop` plus EIS — is reachable.
    pub fn usable(&self) -> bool {
        self.remote_desktop_version >= MIN_REMOTE_DESKTOP_VERSION
            && self.device_types & DEVICES_WANTED == DEVICES_WANTED
            && self.screen_cast_version > 0
    }

    /// Whether the compositor can report the pointer position through
    /// screencast metadata — which is what a Wayland kill switch would
    /// need eyes from. Reported, not used: see `inject`'s `cursor`.
    pub fn cursor_metadata(&self) -> bool {
        self.cursor_modes & 4 != 0
    }
}

/// Ask the portal what it supports. Never prompts, never creates a
/// session.
pub fn capabilities() -> Result<Capabilities> {
    let connection = Connection::session().context("cannot reach the session bus")?;
    let remote = proxy(&connection, REMOTE_DESKTOP)?;
    let cast = proxy(&connection, SCREEN_CAST)?;
    Ok(Capabilities {
        remote_desktop_version: remote.get_property("version").unwrap_or(0),
        screen_cast_version: cast.get_property("version").unwrap_or(0),
        device_types: remote.get_property("AvailableDeviceTypes").unwrap_or(0),
        cursor_modes: cast.get_property("AvailableCursorModes").unwrap_or(0),
        have_stored_token: stored_token().is_some(),
    })
}

/// Negotiate a grant and connect to EIS.
///
/// Raises the system consent dialog the first time, and not afterwards:
/// the returned `restore_token` is stored and replayed. A token the
/// compositor has revoked is not an error to work around — the dialog
/// simply appears again, which is the correct outcome.
pub fn grant() -> Result<Grant> {
    let connection = Connection::session().context("cannot reach the session bus")?;
    let remote = proxy(&connection, REMOTE_DESKTOP)?;
    let cast = proxy(&connection, SCREEN_CAST)?;

    let version: u32 = remote.get_property("version").unwrap_or(0);
    if version < MIN_REMOTE_DESKTOP_VERSION {
        bail!(
            "this desktop's portal speaks RemoteDesktop version {version}, and \
             synthesizing input needs {MIN_REMOTE_DESKTOP_VERSION} or newer for \
             ConnectToEIS. Update xdg-desktop-portal and the backend for your \
             desktop (xdg-desktop-portal-gnome or -kde)"
        );
    }

    let session = create_session(&connection, &remote)?;
    select_devices(&connection, &remote, &session)?;
    select_sources(&connection, &cast, &session)?;
    let results = start(&connection, &remote, &session)?;

    // Store whatever came back before anything else can fail: a token
    // earned by a dialog the user already answered should not be lost to
    // a later error.
    if let Some(token) = string_in(&results, "restore_token") {
        store_token(&token);
    }

    let fd: zbus::zvariant::OwnedFd = remote
        .call("ConnectToEIS", &(&session, HashMap::<&str, Value>::new()))
        .context(
            "the portal would not open an EIS connection. This is the path GNOME and KDE \
             use; on a compositor without it, input synthesis is not available yet",
        )?;
    let eis = UnixStream::from(std::os::fd::OwnedFd::from(fd));

    Ok(Grant {
        eis: Some(eis),
        _session: session,
        _connection: connection,
    })
}

fn proxy<'a>(connection: &Connection, interface: &'a str) -> Result<Proxy<'a>> {
    Proxy::new(connection, DEST, OBJECT, interface)
        .with_context(|| format!("cannot talk to {interface} — is xdg-desktop-portal running?"))
}

/// Every portal method answers twice: an object path immediately, then the
/// real result as a `Response` signal on it. This is that pattern, and the
/// subscription has to exist *before* the call — the signal can arrive
/// before the method reply does.
struct Pending {
    messages: MessageIterator,
    what: &'static str,
}

impl Pending {
    fn subscribe(connection: &Connection, path: &str, what: &'static str) -> Result<Self> {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.portal.Request")?
            .member("Response")?
            .path(path.to_string())?
            .build();
        Ok(Self {
            messages: MessageIterator::for_match_rule(rule, connection, None)
                .with_context(|| format!("cannot listen for the {what} response"))?,
            what,
        })
    }

    fn wait(mut self) -> Result<Results> {
        let what = self.what;
        let message = self
            .messages
            .next()
            .ok_or_else(|| anyhow!("the bus closed before {what} answered"))?
            .with_context(|| format!("reading the {what} response"))?;
        let (code, results) = message
            .body()
            .deserialize::<(u32, Results)>()
            .with_context(|| format!("the {what} response was not readable"))?;
        match code {
            0 => Ok(results),
            1 => bail!(
                "the screen-sharing request was declined, so there is nothing to act \
                 through. Run `pixelactions doctor --probe` and approve the dialog"
            ),
            other => bail!("{what} ended without a grant (portal response {other})"),
        }
    }
}

/// Request paths are built by the portal from our bus name and a token we
/// choose, so we can predict the path and subscribe before calling.
fn request_path(connection: &Connection, token: &str) -> Result<String> {
    let unique = connection
        .inner()
        .unique_name()
        .ok_or_else(|| anyhow!("this process has no unique bus name yet"))?
        .to_string();
    let sender = unique.trim_start_matches(':').replace('.', "_");
    Ok(format!("{OBJECT}/request/{sender}/{token}"))
}

/// A token unique to this process and call. Two pixelactions runs at once
/// must not collide on a request path.
fn token(kind: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("pxa_{kind}_{}_{n}", std::process::id())
}

fn create_session(connection: &Connection, remote: &Proxy<'_>) -> Result<OwnedObjectPath> {
    let handle = token("create");
    let pending = Pending::subscribe(
        connection,
        &request_path(connection, &handle)?,
        "CreateSession",
    )?;
    let session_token = token("session");
    let options = HashMap::from([
        ("handle_token", Value::from(handle.as_str())),
        ("session_handle_token", Value::from(session_token.as_str())),
    ]);
    let _: OwnedObjectPath = remote
        .call("CreateSession", &(options,))
        .context("the portal refused to open a remote-desktop session")?;
    let results = pending.wait()?;
    let handle = string_in(&results, "session_handle")
        .ok_or_else(|| anyhow!("the portal opened a session but did not name it"))?;
    OwnedObjectPath::try_from(handle.as_str())
        .with_context(|| format!("the portal named its session {handle:?}, which is not a path"))
}

/// Ask for keyboard and pointer, replaying a stored grant if we have one.
fn select_devices(
    connection: &Connection,
    remote: &Proxy<'_>,
    session: &OwnedObjectPath,
) -> Result<()> {
    let handle = token("devices");
    let pending = Pending::subscribe(
        connection,
        &request_path(connection, &handle)?,
        "SelectDevices",
    )?;
    let saved = stored_token();
    let mut options = HashMap::from([
        ("handle_token", Value::from(handle.as_str())),
        ("types", Value::from(DEVICES_WANTED)),
        ("persist_mode", Value::from(PERSIST_UNTIL_REVOKED)),
    ]);
    if let Some(saved) = &saved {
        options.insert("restore_token", Value::from(saved.as_str()));
    }
    let _: OwnedObjectPath = remote
        .call("SelectDevices", &(session, options))
        .context("the portal refused a keyboard and pointer")?;
    pending.wait().map(|_| ())
}

/// Link a screencast session to the same handle. This is what gives the
/// EIS devices their regions, and therefore what makes absolute placement
/// possible at all.
fn select_sources(
    connection: &Connection,
    cast: &Proxy<'_>,
    session: &OwnedObjectPath,
) -> Result<()> {
    let handle = token("sources");
    let pending = Pending::subscribe(
        connection,
        &request_path(connection, &handle)?,
        "SelectSources",
    )?;
    let options = HashMap::from([
        ("handle_token", Value::from(handle.as_str())),
        ("types", Value::from(SOURCE_MONITOR)),
        // Every monitor, so a session spanning displays can be acted on.
        ("multiple", Value::from(true)),
    ]);
    let _: OwnedObjectPath = cast.call("SelectSources", &(session, options)).context(
        "the portal refused to link a screen to the session, which is what absolute \
             pointer placement is measured against",
    )?;
    pending.wait().map(|_| ())
}

fn start(
    connection: &Connection,
    remote: &Proxy<'_>,
    session: &OwnedObjectPath,
) -> Result<Results> {
    let handle = token("start");
    let pending = Pending::subscribe(connection, &request_path(connection, &handle)?, "Start")?;
    let options = HashMap::from([("handle_token", Value::from(handle.as_str()))]);
    let _: OwnedObjectPath = remote
        .call("Start", &(session, "", options))
        .context("the portal refused to start the session")?;
    pending.wait()
}

fn string_in(results: &Results, key: &str) -> Option<String> {
    String::try_from(results.get(key)?.clone()).ok()
}

/// Where the replayable grant lives. `XDG_STATE_HOME` is the right home
/// for it: it is neither configuration a human edits nor a cache that can
/// be cleared without consequence.
fn token_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(base.join("pixelactions").join("wayland-restore-token"))
}

fn stored_token() -> Option<String> {
    let text = std::fs::read_to_string(token_path()?).ok()?;
    let token = text.trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// Store the token, quietly. Failing to persist it costs one extra dialog
/// next run — not worth failing a run that is otherwise granted and
/// about to work.
fn store_token(token: &str) {
    let Some(path) = token_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, token);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capability_check_needs_eis_and_both_device_types() {
        let ok = Capabilities {
            remote_desktop_version: 2,
            screen_cast_version: 5,
            device_types: 7,
            cursor_modes: 7,
            have_stored_token: false,
        };
        assert!(ok.usable());
        assert!(ok.cursor_metadata());

        // Version 1 has no ConnectToEIS.
        let old = Capabilities {
            remote_desktop_version: 1,
            ..ok
        };
        assert!(!old.usable());
        // Pointer offered but no keyboard.
        let pointer_only = Capabilities {
            device_types: 2,
            ..ok
        };
        assert!(!pointer_only.usable());
        // No ScreenCast at all means no regions to aim inside.
        let no_cast = Capabilities {
            screen_cast_version: 0,
            ..ok
        };
        assert!(!no_cast.usable());
    }

    #[test]
    fn cursor_metadata_is_reported_only_when_the_mode_is_offered() {
        let base = Capabilities {
            remote_desktop_version: 2,
            screen_cast_version: 5,
            device_types: 7,
            cursor_modes: 3, // hidden | embedded, no metadata
            have_stored_token: false,
        };
        assert!(!base.cursor_metadata());
        assert!(
            Capabilities {
                cursor_modes: 4,
                ..base
            }
            .cursor_metadata()
        );
    }

    #[test]
    fn tokens_are_unique_per_call_so_two_runs_cannot_collide() {
        let first = token("start");
        let second = token("start");
        assert_ne!(first, second);
        // The pid is in there, which is what separates two processes.
        assert!(first.contains(&std::process::id().to_string()));
    }

    #[test]
    fn the_token_path_sits_under_the_state_directory() {
        // SAFETY-free: reads only what it just set.
        unsafe { std::env::set_var("XDG_STATE_HOME", "/tmp/pxa-state-test") };
        let path = token_path().expect("a state path");
        assert_eq!(
            path,
            PathBuf::from("/tmp/pxa-state-test/pixelactions/wayland-restore-token")
        );
        unsafe { std::env::remove_var("XDG_STATE_HOME") };
    }
}
