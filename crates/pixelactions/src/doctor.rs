//! `doctor` — what this machine can and cannot do, before you need it.
//!
//! Permissions are not "setup friction" to be discovered at the worst
//! moment; they are part of the contract. This reports them plainly,
//! including the ones this build has not implemented yet.

use anyhow::Result;
use serde::Serialize;

use crate::session::SUPPORTED_SCHEMA;

/// The minimum pixelcoords this build can trust, and the reason it is not
/// simply "whatever is installed".
///
/// Below 0.1.2, captures composited the mouse pointer into the image. This
/// tool parks the pointer on whatever it just clicked, so the pointer
/// lands inside the very region the next check re-locates — costing enough
/// match score on a low-detail region to push a perfect match under the
/// floor. The result is a loop that fails intermittently and blames the
/// screen. Refusing an old pixelcoords is cheaper than debugging that.
pub const MIN_PIXELCOORDS: &str = "0.7.7";

/// Split `0.1.2` into comparable numbers. Anything that is not three
/// dotted integers is unreadable rather than assumed good.
fn parts(version: &str) -> Option<(u32, u32, u32)> {
    let mut fields = version.trim().split('.');
    let major = fields.next()?.parse().ok()?;
    let minor = fields.next()?.parse().ok()?;
    // Tolerate a pre-release suffix: 0.1.2-rc1 is 0.1.2 for this purpose.
    let patch = fields.next()?.split(['-', '+']).next()?.parse().ok()?;
    if fields.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Whether an installed version is new enough.
pub fn meets_minimum(found: &str) -> bool {
    let (Some(found), Some(needed)) = (parts(found), parts(MIN_PIXELCOORDS)) else {
        return false;
    };
    found >= needed
}

/// Refuse before acting when the pixelcoords on PATH cannot be trusted.
///
/// Checked once per run rather than per call: this shells out to another
/// binary, and the answer cannot change mid-run.
/// Refuse to act when this process is not per-monitor-DPI-aware on
/// Windows.
///
/// `main` declares the awareness at startup best-effort and discards the
/// result. That is fine for `plan` and `doctor`, which only report — but
/// **acting** on coordinates the OS is silently rescaling against the
/// primary monitor's scale means clicking somewhere other than the region
/// a human marked, on any display that is not at 100%.
///
/// An external compatibility override can force a different mode, so the
/// declaration succeeding is not proof. This asks what actually holds.
///
/// Everywhere else is `Ok`: macOS and Linux have no equivalent to get
/// wrong.
pub fn require_dpi_awareness() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    if !crate::win::is_per_monitor_aware_v2() {
        return Err(
            "this process is not per-monitor-DPI-aware, so Windows is rescaling every \
             coordinate against the primary monitor — a click would land somewhere other \
             than the region that was marked on any display not at 100%. Something \
             overrode the awareness this process declares at startup, most likely a \
             compatibility setting on the executable or the terminal launching it. \
             `pixelactions doctor` reports what is in force"
                .to_string(),
        );
    }
    Ok(())
}

pub fn require_supported_pixelcoords() -> Result<(), String> {
    let status = pixelcoords_status();
    if !status.found {
        return Err(
            "pixelcoords is not on PATH — it is what relocates and verifies regions. \
             Install it with `cargo install pixelcoords`"
                .to_string(),
        );
    }
    let Some(version) = status.version else {
        return Err(
            "could not read `pixelcoords --version`, so its version cannot be trusted. \
             Reinstall with `cargo install pixelcoords`"
                .to_string(),
        );
    };
    if !meets_minimum(&version) {
        return Err(format!(
            "pixelcoords {version} is too old — this build needs {MIN_PIXELCOORDS} or newer. \
             Older captures composite the mouse pointer into the image, which makes \
             relocation unreliable in a way that looks like flakiness. \
             Upgrade with `cargo install pixelcoords`"
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct Report {
    schema: u32,
    platform: &'static str,
    supported_platform: bool,
    /// The coordinate space this platform's input API expects.
    native_space: pixelactions_core::convert::Space,
    session_schema_supported: u32,
    pixelcoords: PixelcoordsStatus,
    /// Where a run's record goes, or `None` when this environment gives
    /// nothing to resolve one from.
    ///
    /// Reported because the log fails to write **quietly** — a run must
    /// not die over it — which means "is it on" is otherwise unanswerable
    /// without going and looking. It was unanswerable on Windows for a
    /// whole release.
    #[serde(skip_serializing_if = "Option::is_none")]
    audit_log: Option<String>,
    /// What this build can actually do today.
    capabilities: Capabilities,
    /// macOS only: whether this process may post synthetic events.
    accessibility_trusted: Option<bool>,
    /// Linux only: which display server this session runs and what its
    /// portal will grant. `None` elsewhere, where the windowing system is
    /// a compile-time fact and there is nothing to discover.
    #[serde(skip_serializing_if = "Option::is_none")]
    linux: Option<LinuxStatus>,
    /// Windows only: the two things that decide whether a coordinate means
    /// what the session says it means, and whether an event will be
    /// delivered at all. `None` elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    windows: Option<WindowsStatus>,
    probe: Probe,
}

/// What a Windows session can do, discovered rather than assumed.
///
/// Windows has no permission to report — nothing is granted, nothing is
/// prompted — so what takes that slot is the two things that actually
/// decide whether a run behaves: which coordinate space the OS is talking
/// in, and whether UIPI will drop the events.
#[derive(Debug, Serialize)]
struct WindowsStatus {
    /// Whether this process is per-monitor-DPI-aware v2. When it is not,
    /// Windows virtualizes every coordinate it reports and accepts against
    /// the primary monitor's scale, so a session's physical pixels and this
    /// process's idea of a pixel are different quantities on any scaled
    /// display. pixelcoords reports the same bit, and the two must agree.
    dpi_aware_v2: bool,
    /// The rectangle absolute mouse events are measured against — the
    /// bounding box of every monitor. Reported because a negative origin is
    /// the normal shape of a left-hand secondary display and the single
    /// most common thing to get wrong.
    virtual_desktop: pixelactions_core::virtualdesk::VirtualDesktop,
    /// Whether this process runs elevated, which is the whole of what UIPI
    /// will and will not allow. `None` when the token could not be read —
    /// "could not tell" is a different answer from "no".
    #[serde(skip_serializing_if = "Option::is_none")]
    elevated: Option<bool>,
}

/// What a Linux session can actually do, discovered rather than assumed.
///
/// The two display servers have nothing in common to report: Wayland's
/// story is a portal and a remembered grant, X11's is a display socket and
/// no permission model at all. So each set of fields is `Option` and
/// **absent** on the other server rather than zeroed — a `0` for a portal
/// version on an X11 session would read as "the portal answered and said
/// zero", which is a different and untrue thing.
#[derive(Debug, Serialize)]
struct LinuxStatus {
    server: pixelactions_core::display::Server,
    /// Which path input would take, named so a bug report can say it.
    /// `none` means this session has no path at all.
    rung: &'static str,
    /// X11: the display this session names, and whether it answered. The
    /// two failure modes on X11 are both environmental, so they are what
    /// gets reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    display: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    connected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    portal_remote_desktop_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    portal_screen_cast_version: Option<u32>,
    /// Bitmask: 1 keyboard, 2 pointer, 4 touchscreen.
    #[serde(skip_serializing_if = "Option::is_none")]
    portal_device_types: Option<u32>,
    /// Whether the compositor could report the pointer position through
    /// screencast metadata. Reported because it is exactly what a Wayland
    /// kill switch would need, and this build does not yet consume it.
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor_metadata_available: Option<bool>,
    /// Whether a previous grant was stored, so no dialog is expected.
    #[serde(skip_serializing_if = "Option::is_none")]
    grant_remembered: Option<bool>,
}

#[derive(Debug, Serialize)]
struct PixelcoordsStatus {
    found: bool,
    version: Option<String>,
    minimum: &'static str,
}

#[derive(Debug, Serialize)]
struct Capabilities {
    resolve: bool,
    inject: bool,
    verify: bool,
}

/// What the probe found, when it ran.
///
/// `moved` and `confirmed` are separate because on Wayland they genuinely
/// differ: the compositor accepts a placement and offers no way to ask
/// where the pointer ended up. Collapsing them would make `doctor` claim
/// a proof it does not have — the same distinction the run report draws
/// between "executed" and "verified".
#[derive(Debug, Serialize)]
struct Probe {
    attempted: bool,
    moved: bool,
    confirmed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// Ask the sister tool its version. Absence is a state to report, not an
/// error — resolving a plan works fine without it.
fn pixelcoords_status() -> PixelcoordsStatus {
    let output = std::process::Command::new("pixelcoords")
        .arg("--version")
        .output();
    let Ok(output) = output else {
        return PixelcoordsStatus {
            found: false,
            version: None,
            minimum: MIN_PIXELCOORDS,
        };
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text.split_whitespace().nth(1).map(str::to_string);
    PixelcoordsStatus {
        found: true,
        version,
        minimum: MIN_PIXELCOORDS,
    }
}

pub fn run(json: bool, probe: bool) -> Result<i32> {
    let probe_result = run_probe(probe);
    // One question, asked once: can this session synthesize input? Both
    // the headline and the capability line come from the same answer, so
    // they cannot disagree.
    let can_inject = crate::inject::availability();
    let report = Report {
        schema: 1,
        platform: std::env::consts::OS,
        supported_platform: can_inject.is_ok(),
        native_space: pixelactions_core::convert::native_space(),
        session_schema_supported: SUPPORTED_SCHEMA,
        pixelcoords: pixelcoords_status(),
        audit_log: crate::audit::log_path().map(|p| p.display().to_string()),
        capabilities: Capabilities {
            resolve: true,
            inject: can_inject.is_ok(),
            verify: true,
        },
        accessibility_trusted: trusted(),
        linux: linux_status(),
        windows: windows_status(),
        probe: probe_result,
    };
    let refusal = can_inject.err();

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(0);
    }

    println!("platform:        {}", report.platform);
    println!(
        "supported:       {}",
        if report.supported_platform {
            "yes".to_string()
        } else {
            // The reason, not a generic no: every refusal here is
            // something the reader can act on.
            format!("no — {}", refusal.as_deref().unwrap_or("unsupported"))
        }
    );
    if let Some(linux) = &report.linux {
        print_linux(linux);
    }
    if let Some(windows) = &report.windows {
        print_windows(windows);
    }
    println!("native space:    {:?}", report.native_space);
    println!(
        "session schema:  {} and older",
        report.session_schema_supported
    );
    match (&report.pixelcoords.found, &report.pixelcoords.version) {
        (true, Some(version)) => {
            let verdict = if meets_minimum(version) {
                "ok"
            } else {
                "TOO OLD"
            };
            println!("pixelcoords:     {version} (minimum {MIN_PIXELCOORDS}) — {verdict}");
        }
        (true, None) => println!("pixelcoords:     found, version unreadable"),
        (false, _) => println!("pixelcoords:     not on PATH — needed to relocate and verify"),
    }
    match &report.audit_log {
        Some(path) => println!("audit log:       {path}"),
        None => println!(
            "audit log:       nowhere — no state directory could be resolved, so runs \
             are not recorded"
        ),
    }
    println!();
    println!("capabilities:");
    println!("  resolve a plan   yes");
    println!("  inject input     {}", inject_line(&report));
    println!("  verify a step    yes — via pixelcoords find");
    if report.probe.attempted {
        println!();
        match (
            report.probe.moved,
            report.probe.confirmed,
            &report.probe.detail,
        ) {
            (true, true, _) => {
                println!("probe:           the cursor moved, and the OS confirmed where it went");
            }
            // Wayland: granted and accepted, but unprovable from here.
            (true, false, detail) => {
                println!("probe:           input was granted and accepted, NOT confirmed");
                if let Some(detail) = detail {
                    println!("  {detail}");
                }
            }
            (false, _, Some(detail)) => println!("probe:           FAILED\n  {detail}"),
            (false, _, None) => println!("probe:           failed, no detail"),
        }
    }
    if report.probe.attempted && !report.probe.moved {
        return Ok(3);
    }
    Ok(0)
}

/// One line naming what a grant costs on this platform, since the answer
/// differs in kind: macOS asks once in System Settings, Wayland asks the
/// user per grant and remembers it, and X11 does not ask at all.
fn inject_line(report: &Report) -> String {
    if !report.capabilities.inject {
        return "no".to_string();
    }
    if let Some(windows) = &report.windows {
        return format!("yes — {}", windows_grant_cost(windows));
    }
    let Some(linux) = &report.linux else {
        return "yes — needs macOS Accessibility permission".to_string();
    };
    // Belt and braces: the capability and the path are discovered by
    // separate calls, and "yes — via none" is a sentence this report must
    // never print. If they disagree, the pessimistic answer is the true one.
    if linux.rung == "none" {
        return "no — this session has no input path".to_string();
    }
    format!("yes — via {}, {}", linux.rung, grant_cost(linux))
}

/// What consent costs on this session, in a phrase.
fn grant_cost(linux: &LinuxStatus) -> &'static str {
    match (linux.server, linux.grant_remembered) {
        (pixelactions_core::display::Server::X11, _) => "which asks nothing of you",
        (_, Some(true)) => "using a remembered screen-share grant",
        _ => "using a screen-share grant you approve once",
    }
}

/// The X11 line: which display, and whether it answered. `None` on any
/// other server, which has no display socket to name.
fn display_line(linux: &LinuxStatus) -> Option<String> {
    let verdict = if linux.connected? {
        "connected"
    } else {
        "no answer"
    };
    let display = linux.display.as_deref().unwrap_or("(unset)");
    Some(format!("{display} — {verdict}"))
}

/// What the portal offers. `None` when nothing asked it — an X11 session,
/// or a Wayland one where the call failed.
fn portal_line(linux: &LinuxStatus) -> Option<String> {
    Some(format!(
        "RemoteDesktop v{} · ScreenCast v{} · devices {:#b}",
        linux.portal_remote_desktop_version?,
        linux.portal_screen_cast_version.unwrap_or(0),
        linux.portal_device_types.unwrap_or(0)
    ))
}

/// Who has to approve, told apart from who already has. "Nothing was
/// remembered" and "there is nothing to remember" are different answers,
/// and on X11 the second one is the security story.
fn grant_line(linux: &LinuxStatus) -> &'static str {
    use pixelactions_core::display::Server;

    match (linux.server, linux.grant_remembered) {
        (Server::X11, _) => {
            "none needed — any X client may inject into any other, which is the hole Wayland closes"
        }
        (_, Some(true)) => "remembered — no dialog expected",
        (_, Some(false)) => "not yet given — the first run will ask",
        (_, None) => "nothing to grant — this session has no input path",
    }
}

/// Whether the corner kill switch has anything to watch. This is the one
/// line where X11 is ahead of Wayland, and the reason is worth printing.
fn kill_switch_line(linux: &LinuxStatus) -> &'static str {
    use pixelactions_core::display::Server;

    match (linux.server, linux.cursor_metadata_available) {
        (Server::X11, _) => "armed — X11 reports the pointer position, so the corner check works",
        (Server::Wayland, Some(true)) => {
            "no eyes on Wayland in this build (the compositor could provide them)"
        }
        (Server::Wayland, _) => "no eyes on Wayland, and this compositor offers no cursor metadata",
        (Server::Unknown, _) => "nothing to watch — no session was found",
    }
}

/// What consent costs on Windows: nothing, and the sentence says so in the
/// same breath as the thing it does cost you, because "no permission
/// needed" on its own reads as "no limits".
fn windows_grant_cost(windows: &WindowsStatus) -> &'static str {
    match windows.elevated {
        Some(true) => {
            "nothing to grant, and this process is elevated, so only the secure desktop \
             (UAC, the login screen) is out of reach"
        }
        Some(false) => {
            "nothing to grant, but UIPI puts elevated windows out of reach — this process \
             is not elevated"
        }
        None => "nothing to grant; UIPI puts elevated windows out of reach",
    }
}

/// Whether the coordinates this process sees are the ones the session
/// recorded. The failure is silent and total on a scaled display, so it is
/// reported before anything else Windows-specific.
fn dpi_line(windows: &WindowsStatus) -> &'static str {
    if windows.dpi_aware_v2 {
        return "per-monitor v2 — scale factors are read per display, so mixed 100%/150% \
                layouts resolve correctly";
    }
    "NOT per-monitor v2 — something overrode it, and Windows is virtualizing every \
     coordinate against the primary monitor's scale. Clicks on a scaled display will be \
     wrong. Check for an app-compatibility override on your terminal"
}

/// The rectangle an absolute event is measured against. Printed with its
/// origin because a negative one is the normal shape of a left-hand
/// secondary display, and seeing it is how a reader confirms the whole
/// desktop is in play rather than the primary monitor alone.
fn desktop_line(windows: &WindowsStatus) -> String {
    let desk = windows.virtual_desktop;
    format!(
        "{} × {} from ({}, {}) — every monitor, via MOUSEEVENTF_VIRTUALDESK",
        desk.width, desk.height, desk.x, desk.y
    )
}

/// The UIPI line, stated as a limit rather than as a warning. It is an OS
/// rule, not a bug and not a setting, and there is no grant that lifts it.
fn uipi_line(windows: &WindowsStatus) -> &'static str {
    match windows.elevated {
        Some(true) => {
            "elevated — this process can drive elevated windows. The UAC dialog and the \
             login screen still cannot be reached by anything"
        }
        Some(false) => {
            "not elevated — input to an elevated window will be dropped by Windows, not by \
             this tool. Run the target unelevated, or run this elevated too"
        }
        None => "unknown — this process's token could not be read",
    }
}

fn print_windows(windows: &WindowsStatus) {
    println!("dpi awareness:   {}", dpi_line(windows));
    println!("virtual desktop: {}", desktop_line(windows));
    println!("elevation:       {}", uipi_line(windows));
    println!(
        "kill switch:     armed — Windows reports the pointer position, so the corner check works"
    );
}

fn print_linux(linux: &LinuxStatus) {
    println!("session:         {}", linux.server.name());
    println!("input path:      {}", linux.rung);
    if let Some(display) = display_line(linux) {
        println!("display:         {display}");
    }
    if let Some(portal) = portal_line(linux) {
        println!("portal:          {portal}");
    }
    println!("grant:           {}", grant_line(linux));
    println!("kill switch:     {}", kill_switch_line(linux));
}

/// A session with no input path at all — the shape every other branch
/// falls back to, so a new field cannot be forgotten in one place.
#[cfg(any(target_os = "linux", test))]
fn no_input_path(server: pixelactions_core::display::Server) -> LinuxStatus {
    LinuxStatus {
        server,
        rung: "none",
        display: None,
        connected: None,
        portal_remote_desktop_version: None,
        portal_screen_cast_version: None,
        portal_device_types: None,
        cursor_metadata_available: None,
        grant_remembered: None,
    }
}

/// What this Linux session offers. `None` off Linux.
#[cfg(target_os = "linux")]
fn linux_status() -> Option<LinuxStatus> {
    use pixelactions_core::display::Server;

    let server = crate::inject::session_server();
    let status = match server {
        Server::X11 => x11_status(),
        Server::Wayland => wayland_status(),
        Server::Unknown => no_input_path(server),
    };
    Some(status)
}

/// Both X11 failure modes are environmental, so both get reported: which
/// display was tried, and whether it answered.
///
/// Connecting is safe to do unasked, which is exactly the point being
/// reported — XTEST grants nothing and prompts nobody, so the connection
/// is opened and dropped on the spot.
#[cfg(target_os = "linux")]
fn x11_status() -> LinuxStatus {
    use pixelactions_core::display::Server;

    let connected = crate::inject::X11Injector::new().is_ok();
    LinuxStatus {
        rung: if connected {
            "XTEST on the root window"
        } else {
            "none"
        },
        display: std::env::var("DISPLAY")
            .ok()
            .filter(|value| !value.trim().is_empty()),
        connected: Some(connected),
        ..no_input_path(Server::X11)
    }
}

/// Asking the portal is cheap and prompts nothing, so `doctor` asks rather
/// than guesses.
#[cfg(target_os = "linux")]
fn wayland_status() -> LinuxStatus {
    use pixelactions_core::display::Server;

    let Ok(portal) = crate::portal::capabilities() else {
        return no_input_path(Server::Wayland);
    };
    LinuxStatus {
        rung: if portal.usable() {
            "portal RemoteDesktop + EIS"
        } else {
            "none"
        },
        portal_remote_desktop_version: Some(portal.remote_desktop_version),
        portal_screen_cast_version: Some(portal.screen_cast_version),
        portal_device_types: Some(portal.device_types),
        cursor_metadata_available: Some(portal.cursor_metadata()),
        grant_remembered: Some(portal.have_stored_token),
        ..no_input_path(Server::Wayland)
    }
}

#[cfg(not(target_os = "linux"))]
fn linux_status() -> Option<LinuxStatus> {
    None
}

/// What this Windows machine will actually do with a coordinate. Every
/// field is asked rather than assumed, and none of it prompts.
#[cfg(target_os = "windows")]
fn windows_status() -> Option<WindowsStatus> {
    Some(WindowsStatus {
        dpi_aware_v2: crate::win::is_per_monitor_aware_v2(),
        virtual_desktop: crate::win::virtual_desktop(),
        elevated: crate::win::is_elevated(),
    })
}

#[cfg(not(target_os = "windows"))]
fn windows_status() -> Option<WindowsStatus> {
    None
}

/// Whether this process may post synthetic events. `None` off macOS,
/// which has no equivalent state to report.
fn trusted() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        Some(crate::mac::is_trusted())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Try a harmless one-pixel cursor move, when asked.
#[cfg(target_os = "macos")]
fn run_probe(requested: bool) -> Probe {
    if !requested {
        return Probe {
            attempted: false,
            moved: false,
            confirmed: false,
            detail: None,
        };
    }
    // Without the grant, macOS discards synthetic events silently. Ask
    // for it — the system dialog is the only thing that adds the calling
    // application to the Accessibility list, which is what a first-time
    // user actually needs.
    if !crate::mac::is_trusted() {
        crate::mac::request_trust();
        return Probe {
            attempted: true,
            moved: false,
            confirmed: false,
            detail: Some(
                "Accessibility is not granted to the application running pixelactions, \
                 so synthetic events would be discarded silently. A system dialog was \
                 just requested — approve it, or add the app under System Settings > \
                 Privacy & Security > Accessibility, then quit and reopen it and run \
                 this again. The grant attaches to the app you launched from (your \
                 terminal), not to the pixelactions binary."
                    .to_string(),
            ),
        };
    }
    let outcome = crate::inject::RealInjector::new().and_then(|mut injector| {
        use crate::inject::Injector;
        injector.probe()
    });
    match outcome {
        Ok(()) => Probe {
            attempted: true,
            moved: true,
            // macOS can be asked where the cursor ended up, so this is a
            // real proof rather than an acceptance.
            confirmed: true,
            detail: None,
        },
        Err(error) => Probe {
            attempted: true,
            moved: false,
            confirmed: false,
            detail: Some(format!("{error:#}")),
        },
    }
}

/// The two Linux paths can prove different amounts, so they are probed
/// differently rather than reported as if they were the same.
#[cfg(target_os = "linux")]
fn run_probe(requested: bool) -> Probe {
    use pixelactions_core::display::Server;

    if !requested {
        return Probe {
            attempted: false,
            moved: false,
            confirmed: false,
            detail: None,
        };
    }
    if let Err(reason) = crate::inject::availability() {
        return Probe {
            attempted: true,
            moved: false,
            confirmed: false,
            detail: Some(reason),
        };
    }
    match crate::inject::session_server() {
        Server::X11 => probe_x11(),
        // Unknown never reaches here: availability refused it above.
        _ => probe_wayland(),
    }
}

/// X11 gets the real proof: read the cursor, move it one pixel, ask the
/// server where it ended up, put it back.
///
/// This is the same check macOS runs, and it can run here for the same
/// reason — X11 will answer where the pointer is. `XSync` alone would only
/// prove the server *processed* a fake event, which is not the same as
/// having acted on it.
#[cfg(target_os = "linux")]
fn probe_x11() -> Probe {
    let outcome = crate::inject::X11Injector::new().and_then(|mut injector| {
        use crate::inject::Injector;
        injector.probe()
    });
    match outcome {
        Ok(()) => Probe {
            attempted: true,
            moved: true,
            // Read back from the server, so this is proof rather than
            // acceptance — the one Linux path that can set both.
            confirmed: true,
            detail: None,
        },
        Err(error) => Probe {
            attempted: true,
            moved: false,
            confirmed: false,
            detail: Some(format!("{error:#}")),
        },
    }
}

/// On Wayland the probe is where the consent dialog belongs: at setup
/// time, answered by a human who is present, rather than in the middle of
/// a run that is not being watched.
///
/// What it can establish: the portal granted a session, the compositor
/// offered a pointer that takes coordinates, and it described a region to
/// aim inside. What it cannot: that the pointer moved — nothing on Wayland
/// will say. That gap is reported rather than papered over.
#[cfg(target_os = "linux")]
fn probe_wayland() -> Probe {
    // No monitors: the probe never places a pointer, so it needs no
    // session. Anything that does need one refuses without it.
    let outcome = crate::inject::WaylandInjector::new(&[]).and_then(|mut injector| {
        use crate::inject::Injector;
        injector.probe().map(|()| {
            let regions = injector.regions().len();
            let typing = injector.can_type();
            (regions, typing)
        })
    });
    match outcome {
        Ok((regions, typing)) => Probe {
            attempted: true,
            moved: true,
            confirmed: false,
            detail: Some(format!(
                "the compositor granted input and described {regions} region(s); typing is \
                 {}. Whether the pointer moved cannot be checked — Wayland exposes no way \
                 to ask where it is, which is also why the corner kill switch has nothing \
                 to watch here",
                if typing {
                    "available"
                } else {
                    "unavailable (no keymap was sent)"
                }
            )),
        },
        Err(error) => Probe {
            attempted: true,
            moved: false,
            confirmed: false,
            detail: Some(format!("{error:#}")),
        },
    }
}

/// Windows gets the real proof, for the same reason macOS and X11 do: it
/// will answer where the pointer is. Read the cursor, move it one pixel,
/// ask again, put it back.
///
/// There is no permission dialog to raise here — nothing to ask for, and
/// nothing that would grant more. What this can still catch is the failure
/// that matters on Windows: a higher-integrity window holding the input
/// desktop, where `SendInput` returns and nothing moves.
#[cfg(target_os = "windows")]
fn run_probe(requested: bool) -> Probe {
    if !requested {
        return Probe {
            attempted: false,
            moved: false,
            confirmed: false,
            detail: None,
        };
    }
    let outcome = crate::inject::WindowsInjector::new().and_then(|mut injector| {
        use crate::inject::Injector;
        injector.probe()
    });
    match outcome {
        Ok(()) => Probe {
            attempted: true,
            moved: true,
            // Read back from the OS, so this is proof rather than
            // acceptance.
            confirmed: true,
            detail: None,
        },
        Err(error) => Probe {
            attempted: true,
            moved: false,
            confirmed: false,
            detail: Some(format!("{error:#}")),
        },
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn run_probe(requested: bool) -> Probe {
    Probe {
        attempted: requested,
        moved: false,
        confirmed: false,
        detail: requested
            .then(|| "input synthesis is not implemented for this platform yet".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_declared_minimum_is_itself_readable() {
        assert!(parts(MIN_PIXELCOORDS).is_some(), "{MIN_PIXELCOORDS}");
    }

    /// CI downloads a pinned pixelcoords to run the scenarios against, and
    /// that pin has to be a version this build will accept.
    ///
    /// Raising the floor without moving the workflow makes every
    /// display-dependent job fail at once with "0.7.0 is too old" -- which
    /// is the floor working, but it costs a full CI round to find out.
    /// Cheaper to fail here, in milliseconds.
    #[test]
    fn ci_installs_a_pixelcoords_this_build_accepts() {
        const WORKFLOW: &str = include_str!("../../../.github/workflows/ci.yml");

        let pinned: Vec<&str> = WORKFLOW
            .split("pixelcoords/releases/download/v")
            .skip(1)
            .filter_map(|rest| rest.split(['/', '\n', ' ']).next())
            .collect();
        assert!(
            !pinned.is_empty(),
            "the workflow no longer downloads pixelcoords -- update this test"
        );

        for version in pinned {
            assert!(
                meets_minimum(version),
                "CI installs pixelcoords {version}, which this build refuses \
                 (minimum {MIN_PIXELCOORDS})"
            );
        }
    }

    #[test]
    fn newer_and_equal_versions_are_accepted() {
        assert!(meets_minimum(MIN_PIXELCOORDS));
        assert!(meets_minimum("0.7.8"));
        assert!(meets_minimum("0.8.0"));
        assert!(meets_minimum("1.0.0"));
        // A pre-release of the minimum still carries the fix.
        assert!(meets_minimum("0.7.7-rc1"));
    }

    /// The versions refused here are the ones that were *accepted* before
    /// the minimum moved to 0.7.7.
    ///
    /// The 0.7.x entries are the interesting ones: `resolve`, `wait` and
    /// `diff` all exist in them, so the pairing runs — it just runs against
    /// a `resolve` that answers for regions with no interior instead of
    /// refusing them, which 0.7.3 fixed. This project supports the latest
    /// patch and says so rather than blessing a pairing it has not tested.
    #[test]
    fn older_versions_are_refused() {
        assert!(!meets_minimum("0.7.6"));
        assert!(!meets_minimum("0.7.0"));
        assert!(!meets_minimum("0.6.0"));
        assert!(!meets_minimum("0.5.3"));
        assert!(!meets_minimum("0.1.2"));
        assert!(!meets_minimum("0.0.9"));
    }

    #[test]
    fn an_unreadable_version_is_refused_rather_than_assumed_good() {
        for bad in ["", "0.1", "0.1.2.3", "banana", "v0.1.2", "0.x.2"] {
            assert!(!meets_minimum(bad), "should refuse {bad:?}");
        }
    }

    use pixelactions_core::display::Server;

    /// An X11 session as `x11_status` would build it, without needing one.
    fn x11(connected: bool) -> LinuxStatus {
        LinuxStatus {
            rung: if connected {
                "XTEST on the root window"
            } else {
                "none"
            },
            display: Some(":0".to_string()),
            connected: Some(connected),
            ..no_input_path(Server::X11)
        }
    }

    /// A Windows machine as `windows_status` would build it: two monitors
    /// with the secondary placed left of the primary, which is the layout
    /// whose negative origin the virtual-desktop mapping exists for.
    fn windows(dpi_aware_v2: bool, elevated: Option<bool>) -> WindowsStatus {
        WindowsStatus {
            dpi_aware_v2,
            virtual_desktop: pixelactions_core::virtualdesk::VirtualDesktop {
                x: -1920,
                y: 0,
                width: 3840,
                height: 1080,
            },
            elevated,
        }
    }

    /// A working Wayland session as `wayland_status` would build it.
    fn wayland() -> LinuxStatus {
        LinuxStatus {
            rung: "portal RemoteDesktop + EIS",
            portal_remote_desktop_version: Some(2),
            portal_screen_cast_version: Some(5),
            portal_device_types: Some(0b111),
            cursor_metadata_available: Some(true),
            grant_remembered: Some(true),
            ..no_input_path(Server::Wayland)
        }
    }

    /// The X11 report must not borrow Wayland's story. Every line where the
    /// two platforms genuinely differ is checked, because the failure mode
    /// is a report that reads plausibly and describes the wrong machine.
    #[test]
    fn an_x11_session_is_never_described_as_a_wayland_one() {
        let linux = x11(true);
        assert_eq!(display_line(&linux).as_deref(), Some(":0 — connected"));
        assert!(
            portal_line(&linux).is_none(),
            "nothing asked the portal on X11, so there is no version to print"
        );
        let grant = grant_line(&linux);
        assert!(grant.contains("none needed"), "{grant}");
        let kill = kill_switch_line(&linux);
        assert!(kill.starts_with("armed"), "{kill}");
        assert!(
            !kill.contains("no eyes"),
            "X11 can read the pointer: {kill}"
        );
    }

    /// The kill switch is the one place X11 is ahead, and the report has to
    /// say so in opposite terms on the two servers.
    #[test]
    fn the_kill_switch_line_disagrees_between_the_two_servers() {
        assert_ne!(
            kill_switch_line(&x11(true)),
            kill_switch_line(&wayland()),
            "the whole point of reporting it is that the answer differs"
        );
        assert!(kill_switch_line(&wayland()).contains("no eyes"));
        assert!(
            kill_switch_line(&no_input_path(Server::Unknown)).contains("no session"),
            "a session with no path has nothing to watch either"
        );
    }

    /// A display that did not answer is the common X11 failure, and it has
    /// to be visible rather than implied by a missing capability.
    #[test]
    fn an_x_display_that_did_not_answer_says_so() {
        let line = display_line(&x11(false)).expect("X11 names its display");
        assert!(line.contains("no answer"), "{line}");
    }

    #[test]
    fn a_wayland_session_still_reports_its_portal_and_grant() {
        let linux = wayland();
        let portal = portal_line(&linux).expect("the portal answered");
        assert!(portal.contains("RemoteDesktop v2"), "{portal}");
        assert!(portal.contains("ScreenCast v5"), "{portal}");
        assert!(
            display_line(&linux).is_none(),
            "Wayland has no X display to name"
        );
        assert!(grant_line(&linux).contains("remembered"));
    }

    /// The capability line says what a grant costs, and on X11 the honest
    /// answer is "nothing" — which is the security story, not a feature.
    #[test]
    fn the_capability_line_names_what_each_path_costs() {
        let report = |linux: Option<LinuxStatus>| Report {
            schema: 1,
            platform: "linux",
            supported_platform: true,
            native_space: pixelactions_core::convert::Space::Physical,
            session_schema_supported: SUPPORTED_SCHEMA,
            pixelcoords: pixelcoords_status(),
            audit_log: crate::audit::log_path().map(|p| p.display().to_string()),
            capabilities: Capabilities {
                resolve: true,
                inject: true,
                verify: true,
            },
            accessibility_trusted: None,
            linux,
            windows: None,
            probe: Probe {
                attempted: false,
                moved: false,
                confirmed: false,
                detail: None,
            },
        };
        let x11_line = inject_line(&report(Some(x11(true))));
        assert!(x11_line.contains("XTEST"), "{x11_line}");
        assert!(x11_line.contains("asks nothing of you"), "{x11_line}");

        let wayland_line = inject_line(&report(Some(wayland())));
        assert!(wayland_line.contains("remembered"), "{wayland_line}");

        // The sentence this report must never print. A display that did not
        // answer has no path, whatever the capability flag says.
        let dead = inject_line(&report(Some(x11(false))));
        assert!(!dead.contains("via none"), "{dead}");
        assert!(dead.starts_with("no"), "{dead}");

        // Off Linux there is no session to report, and the macOS answer
        // must survive that.
        let mac_line = inject_line(&report(None));
        assert!(mac_line.contains("Accessibility"), "{mac_line}");

        // Windows takes the same slot with the opposite story: nothing is
        // granted, and the limit is an integrity level rather than a
        // permission. It must not borrow macOS's sentence.
        let mut on_windows = report(None);
        on_windows.platform = "windows";
        on_windows.windows = Some(windows(true, Some(false)));
        let line = inject_line(&on_windows);
        assert!(line.contains("nothing to grant"), "{line}");
        assert!(line.contains("UIPI"), "{line}");
        assert!(!line.contains("Accessibility"), "{line}");
    }

    /// The DPI line is the one that decides whether every other number in
    /// the report means anything, so it has to read as a failure when the
    /// awareness is missing rather than as a note.
    #[test]
    fn a_process_without_per_monitor_awareness_says_the_coordinates_are_wrong() {
        let good = dpi_line(&windows(true, Some(false)));
        assert!(good.contains("per-monitor v2"), "{good}");
        assert!(good.contains("mixed"), "names the case it fixes: {good}");

        let bad = dpi_line(&windows(false, Some(false)));
        assert!(bad.contains("NOT per-monitor v2"), "{bad}");
        assert!(
            bad.contains("wrong"),
            "an unaware process clicks the wrong place, and must say so: {bad}"
        );
    }

    /// A secondary display left of the primary gives the desktop a negative
    /// origin. Printing it is how a reader confirms the whole desktop is in
    /// play rather than the primary monitor alone — the exact failure
    /// `MOUSEEVENTF_VIRTUALDESK` exists to prevent.
    #[test]
    fn the_virtual_desktop_line_shows_a_negative_origin_rather_than_hiding_it() {
        let line = desktop_line(&windows(true, None));
        assert!(line.contains("3840 × 1080"), "{line}");
        assert!(line.contains("(-1920, 0)"), "{line}");
        assert!(line.contains("VIRTUALDESK"), "{line}");
    }

    /// UIPI is an OS limit with no grant behind it, and the two elevation
    /// states have genuinely different consequences. Reporting one sentence
    /// for both would make the report useless in exactly the case someone
    /// runs `doctor` to understand.
    #[test]
    fn the_elevation_line_tells_the_two_states_apart() {
        let unelevated = uipi_line(&windows(true, Some(false)));
        assert!(unelevated.contains("not elevated"), "{unelevated}");
        assert!(
            unelevated.contains("dropped by Windows, not by this tool"),
            "the refusal is the OS's, and saying so is the point: {unelevated}"
        );

        let elevated = uipi_line(&windows(true, Some(true)));
        assert_ne!(elevated, unelevated);
        assert!(
            elevated.contains("can drive elevated windows"),
            "{elevated}"
        );
        assert!(
            elevated.contains("login screen"),
            "even elevated, the secure desktop is out of reach: {elevated}"
        );

        // A token that could not be read is its own answer.
        assert!(uipi_line(&windows(true, None)).contains("unknown"));
    }

    /// Windows can read the pointer, so the kill switch is armed — the same
    /// line X11 gets and the opposite of Wayland's. This is the invariant
    /// AGENTS.md pins: only Wayland carries the exception.
    #[test]
    fn windows_serializes_what_it_has_and_nothing_it_does_not() {
        let json = serde_json::to_string(&windows(true, Some(false))).expect("serializes");
        assert!(json.contains(r#""dpi_aware_v2":true"#), "{json}");
        assert!(json.contains(r#""x":-1920"#), "{json}");
        assert!(json.contains(r#""elevated":false"#), "{json}");

        // An unreadable token is absent rather than false — the same rule
        // the Linux fields follow, for the same reason.
        let json = serde_json::to_string(&windows(true, None)).expect("serializes");
        assert!(!json.contains("elevated"), "{json}");
    }

    /// An X11 session must serialize without portal fields at all. A `0`
    /// there would read as "the portal answered and said zero".
    #[test]
    fn absent_fields_are_omitted_rather_than_zeroed() {
        let json = serde_json::to_string(&x11(true)).expect("serializes");
        assert!(json.contains(r#""server":"x11""#), "{json}");
        assert!(json.contains(r#""connected":true"#), "{json}");
        assert!(!json.contains("portal_"), "{json}");
        assert!(!json.contains("grant_remembered"), "{json}");

        let json = serde_json::to_string(&wayland()).expect("serializes");
        assert!(
            json.contains(r#""portal_remote_desktop_version":2"#),
            "{json}"
        );
        assert!(!json.contains("connected"), "{json}");
    }
}
