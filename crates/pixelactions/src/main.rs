//! pixelactions — execute desktop interactions from pixelcoords sessions.
//!
//! This build resolves and reports; it does not yet inject input. That
//! order is deliberate: a wrong coordinate is a click in the wrong place,
//! so the resolution has to be provably right before anything moves.

mod cli;
mod doctor;
mod session;

use anyhow::Result;
use clap::Parser;
use pixelactions_core::convert::Space;
use pixelactions_core::flow::Flow;
use pixelactions_core::plan::{Plan, plan};

/// Exit codes are the API (same contract as the sister tool, plus 3):
/// 0 done · 1 a step failed honestly · 2 malformed question · 3 refused.
const EXIT_MALFORMED: i32 = 2;

fn main() {
    let cli = cli::Cli::parse();
    let result = match cli.command {
        cli::Command::Plan { flow, json, space } => run_plan(&flow, json, space.map(Into::into)),
        cli::Command::Doctor { json } => doctor::run(json),
    };
    if let Err(error) = result {
        eprintln!("pixelactions: {error:#}");
        std::process::exit(EXIT_MALFORMED);
    }
}

/// Resolve every step and print the result. Acts on nothing.
fn run_plan(flow_path: &std::path::Path, json: bool, space: Option<Space>) -> Result<()> {
    let source = std::fs::read_to_string(flow_path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", flow_path.display()))?;
    let flow = Flow::parse(&source)?;

    let session_path = expand_home(&flow.session);
    let session = session::load(&session_path)?;
    let space = space.unwrap_or(flow.settings.space);
    let resolved = plan(&flow, &session, space)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&as_json(&flow, &resolved))?
        );
        return Ok(());
    }
    print_human(&flow, &resolved, &session_path);
    Ok(())
}

fn print_human(flow: &Flow, resolved: &Plan, session_path: &std::path::Path) {
    println!("session:  {}", session_path.display());
    println!(
        "settings: relocate={} verify={:?} space={:?} settle={}ms",
        flow.settings.relocate, flow.settings.verify, flow.settings.space, flow.settings.settle_ms
    );
    println!("steps:    {}", resolved.steps.len());
    println!();
    for step in &resolved.steps {
        println!("  {:>2}. {}", step.index + 1, step.summary);
        for point in &step.points {
            println!(
                "      → ({:.0}, {:.0}) {:?} on monitor {} (scale {})",
                point.x, point.y, point.space, point.monitor, point.scale
            );
        }
    }
    println!();
    println!("nothing was executed — this build resolves only");
}

fn as_json(flow: &Flow, resolved: &Plan) -> serde_json::Value {
    serde_json::json!({
        "schema": 1,
        "session": flow.session,
        "settings": {
            "relocate": flow.settings.relocate,
            "space": flow.settings.space,
            "settle_ms": flow.settings.settle_ms,
        },
        "steps": resolved.steps.iter().map(|step| serde_json::json!({
            "index": step.index,
            "summary": step.summary,
            "points": step.points,
        })).collect::<Vec<_>>(),
        "executed": false,
    })
}

/// Expand a leading `~` — flow files are hand-written, and a session path
/// under the home directory is the common case.
fn expand_home(path: &str) -> std::path::PathBuf {
    let Some(rest) = path.strip_prefix("~/") else {
        return std::path::PathBuf::from(path);
    };
    let Some(home) = std::env::var_os("HOME") else {
        return std::path::PathBuf::from(path);
    };
    std::path::PathBuf::from(home).join(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_expansion_handles_the_common_shapes() {
        // SAFETY-free: this only reads the variable it just set.
        unsafe { std::env::set_var("HOME", "/home/tester") };
        assert_eq!(
            expand_home("~/captures/x").to_str(),
            Some("/home/tester/captures/x")
        );
        assert_eq!(
            expand_home("/absolute/path").to_str(),
            Some("/absolute/path")
        );
        assert_eq!(expand_home("relative/path").to_str(), Some("relative/path"));
        // A bare "~" is not a home reference in shell either.
        assert_eq!(expand_home("~").to_str(), Some("~"));
    }
}
