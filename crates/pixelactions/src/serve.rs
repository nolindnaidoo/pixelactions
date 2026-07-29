//! `serve` — the line protocol, wired to the same machinery as `run`.
//!
//! This is the surface that makes pixelactions programmable from any
//! language: a bot spawns this process, writes one JSON object per line,
//! and reads one back. No FFI, no native module, no package to release
//! in lockstep with the binary. The loop — branching, retries, reading a
//! CSV, calling an API — lives in the caller's language, where it
//! belongs.
//!
//! Every request is turned into a one-step [`Flow`] and pushed through
//! [`run::execute`], the same function `run` uses. That is deliberate:
//! bounds enforcement, verification, relocation and the report format
//! cannot drift between the two surfaces, because there is only one
//! implementation of each.
//!
//! Two behaviors differ from a flow-file run, both because a serve
//! session is open-ended where a flow is a fixed list:
//!
//! - **Relocation happens once, lazily, before the first acting step** —
//!   not at startup. A bot that opens with `wait_for` is waiting for a
//!   UI that is not on screen yet, and refusing to start would make that
//!   impossible.
//! - **A missing region only blocks the steps that name it.** A flow's
//!   targets are exactly what it will touch, so `run` can refuse whole;
//!   a session may describe ten regions a given bot never visits.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use pixelactions_core::flow::{Flow, Settings, Step};
use pixelactions_core::plan::plan;
use pixelactions_core::protocol::{
    PROTOCOL_VERSION, RequestBody, Response, ResponseBody, id_in, parse_request, supported_verbs,
};
use pixelcoords_core::session::SessionFile;

use crate::inject::Injector;
use crate::run::{self, Corrections};
use crate::{session, verify};

/// Whether the loop should read another line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    Continue,
    Stop,
}

/// One serve session: a session file, the settings agreed at handshake,
/// and whatever relocation has learned so far.
struct Server<'a> {
    session_path: PathBuf,
    session: SessionFile,
    settings: Settings,
    corrections: Corrections,
    /// Labels the last relocation pass could not confirm. Acting on one
    /// would be acting blind, so steps naming them are refused.
    missing: Vec<String>,
    relocated: bool,
    greeted: bool,
    injector: &'a mut dyn Injector,
}

/// Read requests until stdin closes or the client says `bye`.
pub fn run(session_directory: &Path) -> Result<i32> {
    if !cfg!(target_os = "macos") {
        eprintln!(
            "pixelactions: input synthesis is macOS-only in this build — \
             `plan` works everywhere"
        );
        return Ok(crate::EXIT_REFUSED);
    }
    let session = session::load(session_directory)?;
    let mut injector = crate::make_injector()?;
    let mut server = Server {
        session_path: session_directory.to_path_buf(),
        session,
        settings: Settings::default(),
        corrections: Corrections::new(),
        missing: Vec::new(),
        relocated: false,
        greeted: false,
        injector: injector.as_mut(),
    };

    // Logs go to stderr. stdout carries protocol and nothing else — a
    // stray println here would corrupt every client's parser.
    eprintln!(
        "pixelactions serve: protocol {PROTOCOL_VERSION}, session {}",
        session_directory.display()
    );

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let (response, next) = server.handle(&line);
        // Flush per line: the client is blocked reading this response,
        // and a buffered reply is a deadlock.
        stdout.write_all(response.to_line().as_bytes())?;
        stdout.flush()?;
        if next == Next::Stop {
            break;
        }
    }
    Ok(0)
}

impl Server<'_> {
    fn handle(&mut self, line: &str) -> (Response, Next) {
        let request = match parse_request(line) {
            Ok(request) => request,
            Err(detail) => return (Response::error(id_in(line), detail), Next::Continue),
        };
        let id = request.id;

        let (body, next) = match request.body {
            RequestBody::Hello { version, settings } => {
                (self.hello(version, settings), Next::Continue)
            }
            RequestBody::Bye => (ResponseBody::Closed, Next::Stop),
            other if !self.greeted => (
                ResponseBody::Error {
                    detail: format!(
                        "say hello first: {{\"do\":\"hello\",\"version\":{PROTOCOL_VERSION}}}. \
                         The handshake is what lets this protocol change later without \
                         breaking your program. (got: {})",
                        describe(&other)
                    ),
                },
                Next::Continue,
            ),
            RequestBody::Relocate => (self.relocate_response(), Next::Continue),
            RequestBody::Step { step } => (self.step(step), Next::Continue),
        };
        (Response { id, body }, next)
    }

    /// The handshake. Rejecting a version mismatch here — rather than
    /// letting a client discover it one malformed step at a time — is the
    /// whole reason the handshake exists.
    fn hello(&mut self, version: u32, settings: Option<Settings>) -> ResponseBody {
        if version != PROTOCOL_VERSION {
            return ResponseBody::Error {
                detail: format!(
                    "this build speaks protocol version {PROTOCOL_VERSION}, \
                     you asked for {version} — upgrade whichever side is older"
                ),
            };
        }
        if let Some(settings) = settings {
            self.settings = settings;
        }
        self.greeted = true;
        ResponseBody::Welcome {
            version: PROTOCOL_VERSION,
            verbs: supported_verbs(),
            session: self.session_path.display().to_string(),
        }
    }

    /// Perform one step, through the same path a flow file takes.
    fn step(&mut self, step: Step) -> ResponseBody {
        let targets: Vec<String> = step.targets().iter().map(|t| (*t).to_string()).collect();
        if acts_on_screen(&step)
            && let Err(detail) = self.ensure_relocated(&targets)
        {
            return ResponseBody::Error { detail };
        }

        let flow = Flow {
            session: self.session_path.display().to_string(),
            settings: self.settings.clone(),
            steps: vec![step],
        };
        let resolved = match plan(&flow, &self.session, self.settings.space) {
            Ok(resolved) => resolved,
            Err(error) => {
                return ResponseBody::Error {
                    detail: error.to_string(),
                };
            }
        };

        let mut verifier = |session: &Path, label: Option<&str>| verify::find(session, label);
        let report = run::execute(
            self.injector,
            &run::Context {
                flow: &flow,
                plan: &resolved,
                session: &self.session_path,
                monitors: &self.session.monitors,
                corrections: &self.corrections,
            },
            &mut verifier,
        );

        let Some(step_report) = report.steps.into_iter().next() else {
            return ResponseBody::Error {
                detail: "the run produced no step report — this is a bug".to_string(),
            };
        };
        ResponseBody::Done {
            outcome: step_report.outcome.name().to_string(),
            points: step_report.points,
            detail: step_report.detail,
            elapsed_ms: step_report.elapsed_ms,
        }
    }

    /// Re-locate every region in the session and report what moved.
    ///
    /// Unlike `run`'s preflight this never refuses: reporting a missing
    /// region is the useful answer to "where is everything right now",
    /// and the refusal belongs at the moment something tries to act on it.
    fn relocate_response(&mut self) -> ResponseBody {
        let moved = match self.relocate() {
            Ok(moved) => moved,
            Err(detail) => return ResponseBody::Error { detail },
        };
        ResponseBody::Located {
            moved,
            missing: self.missing.clone(),
        }
    }

    /// One relocation pass: capture, update corrections, remember what
    /// could not be confirmed. Returns the labels that moved.
    fn relocate(&mut self) -> Result<Vec<String>, String> {
        let labels: Vec<String> = self
            .session
            .selections
            .iter()
            .map(|selection| selection.label.clone())
            .collect();
        let report =
            verify::find(&self.session_path, None).map_err(|error| format!("{error:#}"))?;
        let references: Vec<&str> = labels.iter().map(String::as_str).collect();

        self.missing = references
            .iter()
            .filter(|label| !report.is_confirmed(label))
            .map(|label| (*label).to_string())
            .collect();
        self.corrections = run::corrections(
            &report,
            &references,
            &self.session.monitors,
            self.settings.space,
        );
        self.relocated = true;

        let moved = references
            .iter()
            .filter(|label| {
                report
                    .result_for(label)
                    .and_then(|result| result.delta)
                    .is_some_and(|delta| delta.dx != 0 || delta.dy != 0)
            })
            .map(|label| (*label).to_string())
            .collect();
        Ok(moved)
    }

    /// Relocate once before the first acting step, then refuse only the
    /// steps whose own regions are unaccounted for.
    fn ensure_relocated(&mut self, targets: &[String]) -> Result<(), String> {
        if !self.settings.relocate {
            return Ok(());
        }
        if !self.relocated {
            self.relocate()?;
        }
        let blocked: Vec<&str> = targets
            .iter()
            .filter(|target| self.missing.iter().any(|m| m.eq_ignore_ascii_case(target)))
            .map(String::as_str)
            .collect();
        if blocked.is_empty() {
            return Ok(());
        }
        Err(format!(
            "the screen no longer matches the session for {}: wait for it with \
             {{\"do\":\"wait_for\",\"target\":\"...\"}}, or send {{\"do\":\"relocate\"}} \
             once the UI has settled",
            blocked.join(", ")
        ))
    }
}

/// Whether a step moves the mouse or keyboard, as opposed to only
/// looking. Only the acting half needs coordinates it can trust, so only
/// the acting half pays for a relocation pass.
fn acts_on_screen(step: &Step) -> bool {
    matches!(
        step,
        Step::Click { .. }
            | Step::DoubleClick { .. }
            | Step::Drag { .. }
            | Step::Type { .. }
            | Step::Key { .. }
    )
}

/// A request's verb, for error messages.
fn describe(body: &RequestBody) -> String {
    match body {
        RequestBody::Hello { .. } => "hello".to_string(),
        RequestBody::Relocate => "relocate".to_string(),
        RequestBody::Bye => "bye".to_string(),
        RequestBody::Step { step } => step.summary(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pixelactions_core::flow::Verify;

    #[test]
    fn only_acting_steps_need_trustworthy_coordinates() {
        assert!(acts_on_screen(&Step::Click { target: "a".into() }));
        assert!(acts_on_screen(&Step::Type { text: "hi".into() }));
        assert!(acts_on_screen(&Step::Drag {
            from: "a".into(),
            to: "b".into()
        }));
        // Observation only: these read the screen, they never move it.
        assert!(!acts_on_screen(&Step::Verify { target: "a".into() }));
        assert!(!acts_on_screen(&Step::WaitFor { target: "a".into() }));
        assert!(!acts_on_screen(&Step::WaitGone { target: "a".into() }));
        assert!(!acts_on_screen(&Step::Pause { ms: 10 }));
    }

    #[test]
    fn a_verb_is_named_the_way_the_client_wrote_it() {
        assert_eq!(describe(&RequestBody::Bye), "bye");
        assert_eq!(
            describe(&RequestBody::Step {
                step: Step::Click {
                    target: "submit".into()
                }
            }),
            "click submit"
        );
    }

    #[test]
    fn handshake_settings_replace_the_defaults_wholesale() {
        // Settings arrive as one object with serde defaults filling the
        // gaps, so applying them is a replacement, not a merge — which is
        // exactly how a flow file's [settings] table behaves.
        let sent: Settings =
            serde_json::from_str(r#"{"verify":"none","timeout_ms":50}"#).expect("settings parse");
        assert_eq!(sent.verify, Verify::None);
        assert_eq!(sent.timeout_ms, 50);
        assert_eq!(sent.settle_ms, Settings::default().settle_ms);
    }
}
