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
pub const MIN_PIXELCOORDS: &str = "0.1.2";

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
    /// What this build can actually do today.
    capabilities: Capabilities,
    /// macOS only: whether this process may post synthetic events.
    accessibility_trusted: Option<bool>,
    /// Linux only: which display server this session runs and what its
    /// portal will grant. `None` elsewhere, where the windowing system is
    /// a compile-time fact and there is nothing to discover.
    #[serde(skip_serializing_if = "Option::is_none")]
    linux: Option<LinuxStatus>,
    probe: Probe,
}

/// What a Linux session can actually do, discovered rather than assumed.
#[derive(Debug, Serialize)]
struct LinuxStatus {
    server: pixelactions_core::display::Server,
    /// Which path input would take, named so a bug report can say it.
    /// `none` means this session has no path at all.
    rung: &'static str,
    portal_remote_desktop_version: u32,
    portal_screen_cast_version: u32,
    /// Bitmask: 1 keyboard, 2 pointer, 4 touchscreen.
    portal_device_types: u32,
    /// Whether the compositor could report the pointer position through
    /// screencast metadata. Reported because it is exactly what a Wayland
    /// kill switch would need, and this build does not yet consume it.
    cursor_metadata_available: bool,
    /// Whether a previous grant was stored, so no dialog is expected.
    grant_remembered: bool,
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
        capabilities: Capabilities {
            resolve: true,
            inject: can_inject.is_ok(),
            verify: true,
        },
        accessibility_trusted: trusted(),
        linux: linux_status(),
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
                println!("probe:           the cursor moved — input permission is real");
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
/// user per grant and remembers it.
fn inject_line(report: &Report) -> String {
    if !report.capabilities.inject {
        return "no".to_string();
    }
    let Some(linux) = &report.linux else {
        return "yes — needs macOS Accessibility permission".to_string();
    };
    let grant = if linux.grant_remembered {
        "a remembered screen-share grant"
    } else {
        "a screen-share grant you approve once"
    };
    format!("yes — via {} using {grant}", linux.rung)
}

fn print_linux(linux: &LinuxStatus) {
    println!("session:         {}", linux.server.name());
    println!("input path:      {}", linux.rung);
    println!(
        "portal:          RemoteDesktop v{} · ScreenCast v{} · devices {:#b}",
        linux.portal_remote_desktop_version,
        linux.portal_screen_cast_version,
        linux.portal_device_types
    );
    println!(
        "grant:           {}",
        if linux.grant_remembered {
            "remembered — no dialog expected"
        } else {
            "not yet given — the first run will ask"
        }
    );
    println!(
        "kill switch:     {}",
        if linux.cursor_metadata_available {
            "no eyes on Wayland in this build (the compositor could provide them)"
        } else {
            "no eyes on Wayland, and this compositor offers no cursor metadata"
        }
    );
}

/// What this Linux session offers. `None` off Linux.
#[cfg(target_os = "linux")]
fn linux_status() -> Option<LinuxStatus> {
    use pixelactions_core::display::{Server, detect};

    let server = detect(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
    );
    // Asking the portal is cheap and prompts nothing, but it is only
    // meaningful on Wayland; an X11 session has a different path entirely.
    let portal = match server {
        Server::Wayland => crate::portal::capabilities().ok(),
        _ => None,
    };
    let Some(portal) = portal else {
        return Some(LinuxStatus {
            server,
            rung: "none",
            portal_remote_desktop_version: 0,
            portal_screen_cast_version: 0,
            portal_device_types: 0,
            cursor_metadata_available: false,
            grant_remembered: false,
        });
    };
    Some(LinuxStatus {
        server,
        rung: if portal.usable() {
            "portal RemoteDesktop + EIS"
        } else {
            "none"
        },
        portal_remote_desktop_version: portal.remote_desktop_version,
        portal_screen_cast_version: portal.screen_cast_version,
        portal_device_types: portal.device_types,
        cursor_metadata_available: portal.cursor_metadata(),
        grant_remembered: portal.have_stored_token,
    })
}

#[cfg(not(target_os = "linux"))]
fn linux_status() -> Option<LinuxStatus> {
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

/// On Wayland the probe is where the consent dialog belongs: at setup
/// time, answered by a human who is present, rather than in the middle of
/// a run that is not being watched.
///
/// What it can establish: the portal granted a session, the compositor
/// offered a pointer that takes coordinates, and it described a region to
/// aim inside. What it cannot: that the pointer moved — nothing on Wayland
/// will say. That gap is reported rather than papered over.
#[cfg(target_os = "linux")]
fn run_probe(requested: bool) -> Probe {
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
    // No monitors: the probe never places a pointer, so it needs no
    // session. Anything that does need one refuses without it.
    let outcome = crate::inject::RealInjector::new(&[]).and_then(|mut injector| {
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

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
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

    #[test]
    fn newer_and_equal_versions_are_accepted() {
        assert!(meets_minimum(MIN_PIXELCOORDS));
        assert!(meets_minimum("0.1.3"));
        assert!(meets_minimum("0.2.0"));
        assert!(meets_minimum("1.0.0"));
        // A pre-release of the minimum still carries the fix.
        assert!(meets_minimum("0.1.2-rc1"));
    }

    #[test]
    fn older_versions_are_refused() {
        assert!(!meets_minimum("0.1.1"));
        assert!(!meets_minimum("0.1.0"));
        assert!(!meets_minimum("0.0.9"));
    }

    #[test]
    fn an_unreadable_version_is_refused_rather_than_assumed_good() {
        for bad in ["", "0.1", "0.1.2.3", "banana", "v0.1.2", "0.x.2"] {
            assert!(!meets_minimum(bad), "should refuse {bad:?}");
        }
    }
}
