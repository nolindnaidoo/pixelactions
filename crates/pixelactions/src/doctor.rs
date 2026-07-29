//! `doctor` — what this machine can and cannot do, before you need it.
//!
//! Permissions are not "setup friction" to be discovered at the worst
//! moment; they are part of the contract. This reports them plainly,
//! including the ones this build has not implemented yet.

use anyhow::Result;
use serde::Serialize;

use crate::session::SUPPORTED_SCHEMA;

/// The minimum pixelcoords the loop is built against. Checked, not assumed.
pub const MIN_PIXELCOORDS: &str = "0.1.1";

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

pub fn run(json: bool) -> Result<i32> {
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
            println!("pixelcoords:     {version} (minimum {MIN_PIXELCOORDS})");
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
    Ok(0)
}
