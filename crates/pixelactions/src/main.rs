//! pixelactions — execute desktop interactions from pixelcoords sessions.
//!
//! `plan` resolves and reports without touching anything; `run` performs
//! the flow and confirms each step against a fresh capture. Dry-run is
//! permanent, not a phase — a wrong coordinate is a click in the wrong
//! place, and seeing the numbers first is how that gets caught.

mod cli;
mod doctor;
mod inject;
#[cfg(target_os = "macos")]
mod mac;
mod run;
mod serve;
mod session;
mod verify;

use anyhow::Result;
use clap::Parser;
use pixelactions_core::convert::Space;
use pixelactions_core::flow::Flow;
use pixelactions_core::plan::{Plan, plan};

/// Exit codes are the API (same contract as the sister tool, plus 3):
/// 0 done · 1 a step failed honestly · 2 malformed question · 3 refused.
const EXIT_MALFORMED: i32 = 2;
pub const EXIT_REFUSED: i32 = 3;

fn main() {
    let cli = cli::Cli::parse();
    let result = match cli.command {
        cli::Command::Plan {
            flow,
            session,
            verbs,
            json,
            space,
        } => run_plan(
            &Source {
                flow,
                session,
                verbs,
            },
            json,
            space.map(Into::into),
        ),
        cli::Command::Run {
            flow,
            session,
            verbs,
            json,
            yes,
        } => run_flow(
            &Source {
                flow,
                session,
                verbs,
            },
            json,
            yes,
        ),
        cli::Command::Serve { session } => serve::run(&expand_home(&session.display().to_string())),
        cli::Command::Doctor { json, probe } => doctor::run(json, probe),
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("pixelactions: {error:#}");
            std::process::exit(EXIT_MALFORMED);
        }
    }
}

/// Perform a flow. Returns the process exit code rather than exiting, so
/// the run report is always printed first.
fn run_flow(source: &Source, json: bool, yes: bool) -> Result<i32> {
    if !cfg!(target_os = "macos") {
        eprintln!(
            "pixelactions: input synthesis is macOS-only in this build — \
             `plan` works everywhere"
        );
        return Ok(EXIT_REFUSED);
    }
    // Refuse before acting, not after a confusing failure: relocation and
    // verification both run through the pixelcoords binary.
    if let Err(reason) = doctor::require_supported_pixelcoords() {
        eprintln!("pixelactions: {reason}");
        return Ok(EXIT_REFUSED);
    }
    let (flow, session_path, session) = load_flow(source)?;
    let space = flow.settings.space;
    let resolved = plan(&flow, &session, space)?;

    if !yes {
        eprintln!(
            "about to perform {} steps — this moves your mouse and keyboard.",
            resolved.steps.len()
        );
        eprintln!(
            "run the same arguments with `plan` first to see every coordinate, then pass --yes."
        );
        return Ok(EXIT_REFUSED);
    }

    let mut verifier =
        |session: &std::path::Path, label: Option<&str>| verify::find(session, label);

    // Refuse before acting when the screen has drifted from the capture:
    // clicking coordinates whose regions have moved is vandalism, not
    // automation.
    let corrections = match run::preflight(
        &flow,
        &session_path,
        &session.monitors,
        space,
        &mut verifier,
    ) {
        Ok(corrections) => corrections,
        Err(refusal) => {
            eprintln!("pixelactions: {refusal:#}");
            return Ok(EXIT_REFUSED);
        }
    };
    if !corrections.is_empty() {
        eprintln!(
            "{} region(s) moved since capture — acting on where they are now",
            corrections.len()
        );
    }

    let mut injector = make_injector()?;
    let report = run::execute(
        injector.as_mut(),
        &run::Context {
            flow: &flow,
            plan: &resolved,
            session: &session_path,
            monitors: &session.monitors,
            corrections: &corrections,
            // Preflight just swept every region this run will act on.
            checked: flow.settings.relocate,
        },
        &mut verifier,
    );

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }
    Ok(report.exit_code())
}

#[cfg(target_os = "macos")]
pub fn make_injector() -> Result<Box<dyn inject::Injector>> {
    Ok(Box::new(inject::RealInjector::new()?))
}

#[cfg(not(target_os = "macos"))]
pub fn make_injector() -> Result<Box<dyn inject::Injector>> {
    anyhow::bail!("input synthesis is not implemented for this platform yet")
}

fn print_report(report: &pixelactions_core::report::RunReport) {
    println!("session: {}", report.session);
    println!();
    for step in &report.steps {
        // Padded to a fixed width so the step list reads as a column.
        let mark = format!("{:<8}", step.outcome.name());
        println!(
            "  {} {:>2}. {} ({} ms)",
            mark,
            step.index + 1,
            step.summary,
            step.elapsed_ms
        );
        if let Some(detail) = &step.detail {
            println!("            {detail}");
        }
    }
}

/// Where a run's steps came from: a flow file, or verbs chained on the
/// command line. Both produce the same `Flow`, so everything downstream
/// — resolution, relocation, bounds, verification — is identical, and
/// learning one surface teaches the other.
pub struct Source {
    pub flow: Option<std::path::PathBuf>,
    pub session: Option<std::path::PathBuf>,
    pub verbs: Vec<String>,
}

/// Read a flow and its session together — the pairing every command needs.
fn load_flow(
    source: &Source,
) -> Result<(
    Flow,
    std::path::PathBuf,
    pixelcoords_core::session::SessionFile,
)> {
    let flow = build_flow(source)?;
    let session_path = expand_home(&flow.session);
    let session = session::load(&session_path)?;
    Ok((flow, session_path, session))
}

fn build_flow(source: &Source) -> Result<Flow> {
    if let Some(path) = &source.flow {
        if !source.verbs.is_empty() {
            anyhow::bail!(
                "pass a flow file or chained verbs, not both — the flow already lists its steps"
            );
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
        return Ok(Flow::parse(&text)?);
    }
    let Some(directory) = &source.session else {
        anyhow::bail!("need either --flow FILE or --session DIR with chained verbs");
    };
    if source.verbs.is_empty() {
        anyhow::bail!("nothing to do — pass verbs like click:submit, or --flow with a file");
    }
    Ok(Flow {
        session: directory.display().to_string(),
        settings: pixelactions_core::flow::Settings::default(),
        steps: pixelactions_core::verb::parse_all(&source.verbs)?,
    })
}

/// Resolve every step and print the result. Acts on nothing.
fn run_plan(source: &Source, json: bool, space: Option<Space>) -> Result<i32> {
    let (flow, session_path, session) = load_flow(source)?;
    let space = space.unwrap_or(flow.settings.space);
    let resolved = plan(&flow, &session, space)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&as_json(&flow, &resolved))?
        );
        return Ok(0);
    }
    print_human(&flow, &resolved, &session_path);
    Ok(0)
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
        // Built with `join` rather than spelled out, because the
        // separator it inserts is the platform's own — a literal
        // "/home/tester/captures/x" passes on Unix and fails on Windows,
        // where the correct answer contains a backslash.
        assert_eq!(
            expand_home("~/captures/x"),
            std::path::PathBuf::from("/home/tester").join("captures/x")
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
