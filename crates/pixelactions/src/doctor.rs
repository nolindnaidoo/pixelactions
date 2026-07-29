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
    probe: Probe,
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

/// What the probe found, when it ran. `None` means it was not asked for.
#[derive(Debug, Serialize)]
struct Probe {
    attempted: bool,
    moved: bool,
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
    let report = Report {
        schema: 1,
        platform: std::env::consts::OS,
        supported_platform: cfg!(target_os = "macos"),
        native_space: pixelactions_core::convert::native_space(),
        session_schema_supported: SUPPORTED_SCHEMA,
        pixelcoords: pixelcoords_status(),
        capabilities: Capabilities {
            resolve: true,
            inject: cfg!(target_os = "macos"),
            verify: true,
        },
        accessibility_trusted: trusted(),
        probe: probe_result,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(0);
    }

    println!("platform:        {}", report.platform);
    println!(
        "supported:       {}",
        if report.supported_platform {
            "yes"
        } else {
            "not yet — macOS only in this build"
        }
    );
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
    println!(
        "  inject input     {}",
        if report.capabilities.inject {
            "yes — needs macOS Accessibility permission"
        } else {
            "no  — macOS only in this build"
        }
    );
    println!("  verify a step    yes — via pixelcoords find");
    if report.probe.attempted {
        println!();
        match (&report.probe.moved, &report.probe.detail) {
            (true, _) => println!("probe:           the cursor moved — input permission is real"),
            (false, Some(detail)) => println!("probe:           FAILED\n  {detail}"),
            (false, None) => println!("probe:           failed, no detail"),
        }
    }
    if report.probe.attempted && !report.probe.moved {
        return Ok(3);
    }
    Ok(0)
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
            detail: None,
        },
        Err(error) => Probe {
            attempted: true,
            moved: false,
            detail: Some(format!("{error:#}")),
        },
    }
}

#[cfg(not(target_os = "macos"))]
fn run_probe(requested: bool) -> Probe {
    Probe {
        attempted: requested,
        moved: false,
        detail: requested.then(|| "input synthesis is macOS-only in this build".to_string()),
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
