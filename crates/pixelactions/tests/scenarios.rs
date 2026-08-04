//! What pixelactions does against a real display, on every platform.
//!
//! `scripts/x11-scenarios.sh` proves the one thing only Linux CI can prove:
//! that a synthetic event moves a real pointer, read back from the X server.
//! It is X11-only by construction — `xdotool`, `ImageMagick`, a session written
//! for one unscaled screen — so everything *else* pixelactions does was
//! being proved on one platform out of four.
//!
//! This covers the rest, on macOS, Windows and Linux alike: planning against
//! a session marked from a genuine capture, the refusal a missing `--yes`
//! earns, the exit codes, the line protocol, the MCP surface, and the
//! match-backed verbs. No input is synthesised here. That stays where it can
//! be verified rather than asserted.
//!
//! Off unless `PIXELACTIONS_SCENARIOS` is set, because it needs a display
//! and `pixelcoords` on PATH. `cargo test --workspace` on a laptop skips it.
//!
//! **The region is the whole screen, deliberately.** A smaller one would
//! have to be cut out of the capture, and cropping means an image decoder —
//! a dependency this repo does not have and should not gain for a test. A
//! full-frame region needs no cutting: the crop *is* the screenshot, copied.
//! It also matches at exactly one position, so `find` can never call it
//! ambiguous.

use std::path::{Path, PathBuf};
use std::process::Output;

/// Set by CI, absent on a developer's machine.
fn enabled() -> bool {
    std::env::var("PIXELACTIONS_SCENARIOS").is_ok()
}

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pixelactions"))
}

fn run(args: &[&str]) -> Output {
    std::process::Command::new(binary())
        .args(args)
        .output()
        .expect("pixelactions runs")
}

/// The exit code, with a signal reported as something that is not 0, 1, 2 or
/// 3 — so a killed process can never be mistaken for an answer.
fn code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out))
        .unwrap_or_else(|e| panic!("expected JSON, got {e}:\n{}", stdout(out)))
}

/// A PNG's dimensions, straight out of IHDR.
///
/// The capture is in *physical* pixels and `doctor` reports *logical* ones,
/// so the session cannot be written without reading this. Four bytes at a
/// fixed offset, rather than an image decoder for the same four bytes.
fn png_size(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).expect("the capture is readable");
    assert!(bytes.len() > 24, "{} is not a PNG", path.display());
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{}", path.display());
    let at = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    (at(16), at(20))
}

/// Capture the screen and write the session a human would have saved.
///
/// The monitor comes from `pixelcoords doctor` rather than being invented:
/// `find` matches a session's monitor against the live one by name, so a
/// made-up name means every match-backed scenario fails for a reason that
/// has nothing to do with the code under test.
fn session_over_the_screen(dir: &Path) -> Option<(u32, u32)> {
    let shoot = std::process::Command::new("pixelcoords")
        .args(["shoot", "--out", &dir.display().to_string()])
        .output()
        .expect("pixelcoords is on PATH");
    assert!(
        shoot.status.success(),
        "shoot failed: {}",
        String::from_utf8_lossy(&shoot.stderr)
    );

    let capture = dir.join("screenshot-0.png");
    assert!(capture.exists(), "no capture at {}", capture.display());
    std::fs::copy(&capture, dir.join("crop-0-target.png")).expect("the crop is the capture");
    let (w, h) = png_size(&capture);

    let doctor = std::process::Command::new("pixelcoords")
        .args(["doctor", "--json"])
        .output()
        .expect("pixelcoords doctor runs");
    let doctor: serde_json::Value =
        serde_json::from_slice(&doctor.stdout).expect("doctor speaks JSON");
    let monitor = doctor["monitors"].as_array()?.first()?.clone();
    let scale = monitor["scale"].as_f64().unwrap_or(1.0);
    let name = monitor["name"].as_str().unwrap_or("screen").to_string();

    let px = serde_json::json!({ "x": 0, "y": 0, "w": w, "h": h });
    let session = serde_json::json!({
        "schema": 1,
        "app": { "name": "pixelcoords", "version": "0.7.0" },
        "created_utc": "2026-01-01T00:00:00Z",
        "platform": std::env::consts::OS,
        "capture": null,
        "name": "cross-platform scenarios",
        "monitors": [{
            "index": 0,
            "name": name,
            "primary": true,
            "origin_px": { "x": 0, "y": 0 },
            "size_px": { "w": w, "h": h },
            "scale": scale,
        }],
        "target": null,
        "measures": [],
        "selections": [{
            "shape": "rect", "label": "target", "monitor": 0,
            "px": px, "global_px": px,
            "rot_deg": null, "window_px": null,
            "crop": "crop-0-target.png", "color": null,
        }],
    });
    std::fs::write(
        dir.join("session.json"),
        serde_json::to_vec_pretty(&session).expect("serialises"),
    )
    .expect("session written");
    Some((w, h))
}

static SHARED: std::sync::OnceLock<Option<Capture>> = std::sync::OnceLock::new();
static NEXT_FIXTURE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The one capture every scenario works from.
#[derive(Clone)]
struct Capture {
    dir: PathBuf,
    size: (u32, u32),
}

/// Captured once and copied, never re-captured per test. Sixteen sequential
/// captures is real pressure on a virtual display, and nothing here needs
/// more than one capture of the same still screen.
fn shared_capture() -> Option<&'static Capture> {
    SHARED
        .get_or_init(|| {
            let dir =
                std::env::temp_dir().join(format!("pixelactions-capture-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("capture dir");
            let size = session_over_the_screen(&dir)?;
            Some(Capture { dir, size })
        })
        .as_ref()
}

struct Fixture {
    dir: PathBuf,
    w: u32,
    h: u32,
}

impl Fixture {
    fn path(&self) -> String {
        self.dir.display().to_string()
    }

    /// The centre of a full-screen rect, which is where a click on it lands.
    fn centre(&self) -> (f64, f64) {
        (f64::from(self.w) / 2.0, f64::from(self.h) / 2.0)
    }

    /// A flow file with the corner kill switch off.
    ///
    /// The switch exists so a human can abort a runaway automation by
    /// slamming the pointer into a corner. CI has no human and no control
    /// over where the pointer rests — the macOS runner leaves it at
    /// (10, 10), which is inside the margin — so leaving it on would stop
    /// every run before its first step, and prove nothing about the step.
    fn flow(&self, name: &str, steps: &str) -> String {
        self.write(
            name,
            &format!(
                "session = {:?}\n\n[settings]\nfailsafe = false\n\n{steps}",
                self.path()
            ),
        )
    }

    fn write(&self, name: &str, body: &str) -> String {
        let path = self.dir.join(name);
        std::fs::write(&path, body).expect("fixture file written");
        path.display().to_string()
    }
}

/// A private copy of the shared capture, so a scenario that rewrites the
/// session cannot leak into the next one.
fn fixture() -> Option<Fixture> {
    if !enabled() {
        return None;
    }
    let capture = shared_capture()?;
    let seq = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "pixelactions-scenarios-{}-{seq}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    for entry in std::fs::read_dir(&capture.dir).expect("the capture is readable") {
        let entry = entry.expect("a directory entry");
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            std::fs::copy(entry.path(), dir.join(entry.file_name())).expect("copied");
        }
    }
    Some(Fixture {
        dir,
        w: capture.size.0,
        h: capture.size.1,
    })
}

/// Skip the body when the harness is off, without dressing a skip up as a
/// pass anywhere it matters: CI sets the variable, so CI runs all of it.
macro_rules! scenario {
    ($f:ident) => {
        let Some($f) = fixture() else { return };
    };
}

// ---------------------------------------------------------------------------
// What the tool reports about itself
// ---------------------------------------------------------------------------

/// `doctor` is the first thing anyone runs, and the only place the pairing
/// with pixelcoords is checked. On a runner it should call the platform
/// supported, find the sister binary, and report a resolve capability —
/// which is what every scenario below depends on.
#[test]
fn doctor_reports_a_workable_pairing() {
    if !enabled() {
        return;
    }
    let report = json(&run(&["doctor", "--json"]));

    assert_eq!(
        report["supported_platform"], true,
        "every platform CI runs on is supported: {report}"
    );
    assert_eq!(
        report["pixelcoords"]["found"], true,
        "the workflow installs it: {report}"
    );
    assert_eq!(
        report["capabilities"]["resolve"], true,
        "resolving is what the pairing is for: {report}"
    );
}

/// The floor is a real gate. `doctor` names the minimum it enforces, and
/// the version CI installs must clear it — otherwise every scenario below
/// is running against a pairing the tool itself would reject.
#[test]
fn the_installed_pixelcoords_clears_the_floor() {
    if !enabled() {
        return;
    }
    let report = json(&run(&["doctor", "--json"]));
    let parts = |v: &str| -> Vec<u32> { v.split('.').filter_map(|p| p.parse().ok()).collect() };

    let minimum = report["pixelcoords"]["minimum"]
        .as_str()
        .expect("a minimum");
    let version = report["pixelcoords"]["version"]
        .as_str()
        .expect("a version");
    assert!(
        parts(version) >= parts(minimum),
        "CI installed {version}, the floor is {minimum}: {report}"
    );
}

// ---------------------------------------------------------------------------
// Planning: coordinates, without touching anything
// ---------------------------------------------------------------------------

/// The whole point of the pairing. A session marked from a real capture
/// resolves to a coordinate, and for a rect that coordinate is its centre.
///
/// Compared in the point's *own* space rather than in pixels: the session
/// is written in physical pixels, and `Space::Auto` resolves to logical on
/// macOS and physical elsewhere. Hard-coding either would make this a test
/// of one platform that fails honestly on the others.
#[test]
fn plan_resolves_a_click_to_the_regions_centre() {
    scenario!(f);
    let out = run(&["plan", "--session", &f.path(), "click:target", "--json"]);
    assert_eq!(code(&out), 0, "plan failed: {}", stdout(&out));

    let plan = json(&out);
    let point = &plan["steps"][0]["points"][0];
    let scale = match point["space"].as_str() {
        Some("logical") => point["scale"].as_f64().unwrap_or(1.0),
        _ => 1.0,
    };
    let (x, y) = (
        point["x"].as_f64().expect("an x") * scale,
        point["y"].as_f64().expect("a y") * scale,
    );
    let (want_x, want_y) = f.centre();
    assert!(
        (x - want_x).abs() <= scale && (y - want_y).abs() <= scale,
        "planned {x},{y} in physical pixels, expected the centre {want_x},{want_y}: {plan}"
    );
}

/// Planning touches nothing — that is its contract, and the reason it is
/// safe to hand to a model. A plan against a session leaves no trace in it.
#[test]
fn plan_leaves_the_session_untouched() {
    scenario!(f);
    let session = f.dir.join("session.json");
    let before = std::fs::read(&session).expect("readable");
    let out = run(&["plan", "--session", &f.path(), "click:target", "--json"]);
    assert_eq!(code(&out), 0, "plan failed: {}", stdout(&out));
    let after = std::fs::read(&session).expect("readable");
    assert_eq!(before, after, "plan rewrote the session");
}

/// **Every verb the chain grammar accepts, planned.**
///
/// Not just the ones that take a bare label: `scroll` and `hscroll` need
/// `label>amount`, `drag` needs `from>to`, `type` and `key` take no label
/// at all. A verb the help text advertises and the planner refuses is a
/// promise the tool does not keep, and the ones with unusual grammar are
/// exactly where that goes unnoticed.
#[test]
fn every_verb_the_grammar_accepts_plans() {
    scenario!(f);
    let cases = [
        ("click:target", "click"),
        ("double:target", "double-click"),
        ("verify:target", "verify"),
        ("wait:target", "wait"),
        ("gone:target", "gone"),
        ("changed:target", "changed"),
        ("scroll:target>3", "scroll"),
        ("hscroll:target>-2", "scroll"),
        ("drag:target>target", "drag"),
        ("type:hello", "type"),
        ("key:cmd+s", "key"),
        ("pause:5", "pause"),
    ];

    for (step, expected) in cases {
        let out = run(&["plan", "--session", &f.path(), step, "--json"]);
        assert_eq!(code(&out), 0, "{step} did not plan: {}", stdout(&out));

        let plan = json(&out);
        let summary = plan["steps"][0]["summary"].as_str().unwrap_or_default();
        assert!(
            summary.contains(expected),
            "{step} summarised as {summary:?}, expected it to mention {expected:?}"
        );
    }
}

/// A verb with unusual grammar, given the wrong shape, is refused with the
/// grammar spelled out — `scroll` without its amount is the case a caller
/// actually hits, and being told "scroll needs label>amount" is the
/// difference between a fix and a guess.
#[test]
fn a_verb_given_the_wrong_shape_is_refused_with_its_grammar() {
    scenario!(f);
    let out = run(&["plan", "--session", &f.path(), "scroll:target", "--json"]);
    assert_eq!(code(&out), 2, "{}", stdout(&out));

    let said = format!("{}{}", stdout(&out), String::from_utf8_lossy(&out.stderr));
    assert!(
        said.contains("label>amount") || said.contains('>'),
        "the refusal should show the grammar: {said}"
    );
}

/// Every settings key a flow file may carry is accepted.
///
/// These have no CLI flag — a flow file is the only way to set them — so
/// nothing else in this harness would notice a key that stopped parsing.
/// `failsafe` in particular is the one the match-backed scenarios depend
/// on, and a silent rename would turn the kill switch back on in CI
/// without a single test failing.
#[test]
fn every_settings_key_a_flow_may_carry_is_accepted() {
    scenario!(f);
    let flow = f.write(
        "settings.toml",
        &format!(
            "session = {:?}\n\n[settings]\n\
             relocate = false\n\
             settle_ms = 5\n\
             timeout_ms = 100\n\
             poll_ms = 10\n\
             failsafe = false\n\
             failsafe_margin = 3.0\n\
             audit = false\n\
             \n[[step]]\naction = \"click\"\ntarget = \"target\"\n",
            f.path()
        ),
    );
    let out = run(&["plan", "--flow", &flow, "--json"]);
    assert_eq!(
        code(&out),
        0,
        "a flow using every settings key was refused: {}{}",
        stdout(&out),
        String::from_utf8_lossy(&out.stderr)
    );

    let plan = json(&out);
    assert_eq!(
        plan["settings"]["relocate"], false,
        "the setting reached the plan: {plan}"
    );
}

/// A settings key this build does not have is refused rather than ignored.
/// A silently-dropped setting is one someone believes is in effect — and
/// for `failsafe` that belief is about a safety mechanism.
#[test]
fn an_unknown_settings_key_is_refused() {
    scenario!(f);
    let flow = f.write(
        "bad-settings.toml",
        &format!(
            "session = {:?}\n\n[settings]\nfailsafe_margins = 3.0\n\
             \n[[step]]\naction = \"click\"\ntarget = \"target\"\n",
            f.path()
        ),
    );
    let out = run(&["plan", "--flow", &flow, "--json"]);
    assert_eq!(
        code(&out),
        2,
        "{}{}",
        stdout(&out),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A plan of several steps keeps them in order and resolves each. An
/// executor that reordered steps would be silently wrong.
#[test]
fn a_multi_step_plan_keeps_its_order() {
    scenario!(f);
    let out = run(&[
        "plan",
        "--session",
        &f.path(),
        "verify:target",
        "click:target",
        "verify:target",
        "--json",
    ]);
    assert_eq!(code(&out), 0, "plan failed: {}", stdout(&out));
    let plan = json(&out);
    let summaries: Vec<&str> = plan["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .filter_map(|s| s["summary"].as_str())
        .collect();
    assert_eq!(
        summaries,
        ["verify target", "click target", "verify target"],
        "{plan}"
    );
}

/// A flow file and the same steps typed on the command line are one
/// implementation — the file is parsed into the steps the CLI builds
/// directly, so the resolved coordinates must agree exactly.
#[test]
fn a_flow_file_plans_the_same_as_the_command_line() {
    scenario!(f);
    let flow = f.write(
        "flow.toml",
        &format!(
            "session = {:?}\n\n[[step]]\naction = \"click\"\ntarget = \"target\"\n",
            f.path()
        ),
    );
    let from_file = json(&run(&["plan", "--flow", &flow, "--json"]));
    let from_args = json(&run(&[
        "plan",
        "--session",
        &f.path(),
        "click:target",
        "--json",
    ]));
    assert_eq!(
        from_file["steps"][0]["points"], from_args["steps"][0]["points"],
        "file: {from_file}\nargs: {from_args}"
    );
}

// ---------------------------------------------------------------------------
// Refusals and exit codes: the contract a caller programs against
// ---------------------------------------------------------------------------

/// Without `--yes`, `run` prints what it would do and refuses. This is the
/// safety property the whole tool rests on, and it belongs on every
/// platform rather than on the one that has a virtual X server.
#[test]
fn run_without_yes_refuses_and_says_what_it_would_have_done() {
    scenario!(f);
    let out = run(&["run", "--session", &f.path(), "click:target"]);
    assert_eq!(
        code(&out),
        3,
        "refusal is exit 3: {}{}",
        stdout(&out),
        String::from_utf8_lossy(&out.stderr)
    );
    // A refusal has to leave someone able to proceed, so it names both the
    // safe way to look first and the flag that consents.
    let said = format!("{}{}", stdout(&out), String::from_utf8_lossy(&out.stderr));
    assert!(
        said.contains("plan") && said.contains("--yes"),
        "a refusal must say how to proceed: {said}"
    );
}

/// A label nobody marked is a malformed question, not a negative answer —
/// exit 2, so a caller can tell "your flow is wrong" from "the screen does
/// not match".
#[test]
fn an_unknown_label_exits_two() {
    scenario!(f);
    let out = run(&["plan", "--session", &f.path(), "click:nope", "--json"]);
    assert_eq!(code(&out), 2, "expected 2, got: {}", stdout(&out));
}

/// So is a session that is not there.
#[test]
fn a_missing_session_exits_two() {
    scenario!(f);
    let missing = f.dir.join("no-such-session");
    let out = run(&[
        "plan",
        "--session",
        &missing.display().to_string(),
        "click:target",
        "--json",
    ]);
    assert_eq!(code(&out), 2, "expected 2, got: {}", stdout(&out));
}

/// Flow parsing is strict on purpose — the compatibility contract says
/// session parsing tolerates unknown fields and our own flow parsing does
/// not. A typo'd verb is caught rather than skipped.
#[test]
fn a_flow_with_an_unknown_verb_exits_two() {
    scenario!(f);
    let flow = f.write(
        "bad.toml",
        &format!(
            "session = {:?}\n\n[[step]]\naction = \"clik\"\ntarget = \"target\"\n",
            f.path()
        ),
    );
    let out = run(&["plan", "--flow", &flow, "--json"]);
    assert_eq!(code(&out), 2, "expected 2, got: {}", stdout(&out));
}

// ---------------------------------------------------------------------------
// Match-backed verbs, against the screen as it actually is
// ---------------------------------------------------------------------------

/// Whether the marked region can be matched at all.
///
/// A capture of an empty desktop is a flat color, and pixelcoords refuses
/// to match one — "it matches anywhere rather than somewhere" — which is
/// the correct answer, not a failure. A bare Linux or macOS runner shows
/// exactly that, so the match-backed scenarios below ask first rather than
/// asserting into it and reporting a bug that is not there.
///
/// The question goes to `find`, which is the thing that would have to
/// answer it anyway. Nothing here guesses from pixel values.
fn markable(f: &Fixture) -> bool {
    let out = std::process::Command::new("pixelcoords")
        .args(["find", "--session", &f.path()])
        .output()
        .expect("pixelcoords is on PATH");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let result = &report["results"][0];
    let usable = result["found"] == true && result["ambiguous"] != true;
    if !usable {
        eprintln!(
            "skipped: this display cannot be matched against -- {}",
            result["reason"].as_str().unwrap_or("no reason given")
        );
    }
    usable
}

/// `verify` asks pixelcoords whether the region is still there. Against the
/// screen it was captured from, it is — this proves the whole call-out
/// path, session to `find` to report.
#[test]
fn verify_confirms_the_region_against_a_fresh_capture() {
    scenario!(f);
    if !markable(&f) {
        return;
    }
    let flow = f.flow(
        "verify.toml",
        "[[step]]\naction = \"verify\"\ntarget = \"target\"\n",
    );
    let out = run(&["run", "--flow", &flow, "--yes", "--json"]);
    assert_eq!(
        code(&out),
        0,
        "verify did not confirm: {}{}",
        stdout(&out),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `wait` on a condition that already holds returns at once. The region is
/// on screen, so waiting for it is a formality — but a `wait` that blocked
/// anyway would be a hang, and a hang in CI is a timeout with no diagnosis.
#[test]
fn wait_returns_at_once_when_the_region_is_already_there() {
    scenario!(f);
    if !markable(&f) {
        return;
    }
    let flow = f.flow(
        "wait.toml",
        "[[step]]\naction = \"wait_for\"\ntarget = \"target\"\n",
    );
    let out = run(&["run", "--flow", &flow, "--yes", "--json"]);
    assert_eq!(
        code(&out),
        0,
        "wait did not return: {}{}",
        stdout(&out),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A flat screen is refused, and refused as a *negative answer* rather than
/// an error — exit 1, not 2. This is the case a bare CI desktop actually
/// produces, so it is worth pinning: a caller that cannot tell "nothing to
/// match" from "your flow is wrong" will retry forever.
#[test]
fn an_unmatchable_region_is_a_negative_answer_not_an_error() {
    scenario!(f);
    if markable(&f) {
        return;
    }
    let flow = f.flow(
        "unmatchable.toml",
        "[[step]]\naction = \"verify\"\ntarget = \"target\"\n",
    );
    let out = run(&["run", "--flow", &flow, "--yes", "--json"]);
    assert_eq!(
        code(&out),
        1,
        "an unmatchable region is exit 1: {}{}",
        stdout(&out),
        String::from_utf8_lossy(&out.stderr)
    );
}

// ---------------------------------------------------------------------------
// The other two surfaces
// ---------------------------------------------------------------------------

/// The line protocol is how a program in another language drives this.
/// It agrees a version, says what it can do, and runs a step — all on
/// stdout, one JSON object per line.
#[test]
fn serve_shakes_hands_and_runs_a_step() {
    scenario!(f);
    let hello = serde_json::json!({ "id": 1, "do": "hello", "version": 1 });
    let step = serde_json::json!({ "id": 2, "do": "verify", "target": "target" });
    let bye = serde_json::json!({ "id": 3, "do": "bye" });
    let out = speak(
        &["serve", "--session", &f.path()],
        &format!("{hello}\n{step}\n{bye}\n"),
    );

    let lines: Vec<serde_json::Value> = stdout(&out)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("line {l:?} is not JSON: {e}")))
        .collect();
    assert_eq!(lines.len(), 3, "one response per request: {lines:?}");

    assert_eq!(lines[0]["result"], "welcome", "{}", lines[0]);
    assert_eq!(lines[1]["id"], 2, "responses echo the id: {}", lines[1]);
    assert_eq!(
        lines[2]["result"], "closed",
        "bye closes the session: {}",
        lines[2]
    );
}

/// The handshake tells a client what it may send, and a client is meant to
/// trust it rather than guess. So every verb advertised must be a verb the
/// server will actually take — this is the scenario half of the unit test
/// that caught `scroll` and `changed` missing from the list.
#[test]
fn serve_accepts_every_verb_it_advertises() {
    scenario!(f);
    let hello = serde_json::json!({ "id": 1, "do": "hello", "version": 1 });
    let out = speak(&["serve", "--session", &f.path()], &format!("{hello}\n"));
    let welcome: serde_json::Value = stdout(&out)
        .lines()
        .find(|l| !l.trim().is_empty())
        .and_then(|l| serde_json::from_str(l).ok())
        .unwrap_or_else(|| panic!("no welcome: {}", stdout(&out)));

    let verbs: Vec<String> = welcome["verbs"]
        .as_array()
        .expect("an advertised list")
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    assert!(!verbs.is_empty(), "{welcome}");

    for verb in verbs {
        // Steps deny unknown fields, so each verb gets exactly its own --
        // otherwise a rejection would be about the extra keys rather than
        // about whether the verb is understood.
        let mut request = serde_json::json!({ "id": 9, "do": verb });
        let fields = request.as_object_mut().expect("an object");
        match verb.as_str() {
            "drag" => {
                fields.insert("from".into(), "target".into());
                fields.insert("to".into(), "target".into());
            }
            "scroll" => {
                fields.insert("target".into(), "target".into());
                fields.insert("amount".into(), 1.into());
            }
            "type" => {
                fields.insert("text".into(), "x".into());
            }
            "key" => {
                fields.insert("chord".into(), "a".into());
            }
            "pause" => {
                fields.insert("ms".into(), 1.into());
            }
            _ => {
                fields.insert("target".into(), "target".into());
            }
        }
        let out = speak(
            &["serve", "--session", &f.path()],
            &format!("{hello}\n{request}\n"),
        );
        let reply: Vec<serde_json::Value> = stdout(&out)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| v["id"] == 9)
            .collect();
        let reply = reply
            .first()
            .unwrap_or_else(|| panic!("{verb}: no reply to an advertised verb"));
        let detail = reply["detail"].as_str().unwrap_or_default();
        assert!(
            !detail.contains("unknown") && !detail.contains("cannot read"),
            "{verb} is advertised but not understood: {reply}"
        );
    }
}

/// MCP is the fourth surface on the same implementation, so it must answer
/// the same question the same way. If these ever disagree, one of them has
/// grown its own copy of the resolution path.
#[test]
fn the_mcp_server_plans_the_same_point_as_the_cli() {
    scenario!(f);
    let initialize = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2026-07-28", "capabilities": {},
                    "clientInfo": { "name": "scenarios", "version": "0" } },
    });
    let call = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "pixelactions_plan",
                    "arguments": { "session": f.path(), "steps": ["click:target"] } },
    });
    let out = speak(&["mcp"], &format!("{initialize}\n{call}\n"));
    let replies = jsonrpc_replies(&out);

    let result = replies
        .iter()
        .find(|r| r["id"] == 2)
        .unwrap_or_else(|| panic!("no reply to the call: {replies:?}"));
    assert_eq!(
        result["result"]["isError"],
        serde_json::Value::Bool(false),
        "a plan that resolved is not an error: {result}"
    );
    // The data is in `structuredContent`; `content[0].text` is the sentence
    // a model reads. Both are checked, because a tool that answered only in
    // prose would be unusable to a caller and one that answered only in
    // JSON would be unreadable to the model.
    let plan = &result["result"]["structuredContent"];
    assert!(
        result["result"]["content"][0]["text"]
            .as_str()
            .is_some_and(|t| !t.is_empty()),
        "a tool result with no sentence in it: {result}"
    );
    assert_eq!(plan["ok"], true, "a plan that resolved says so: {plan}");

    let cli = json(&run(&[
        "plan",
        "--session",
        &f.path(),
        "click:target",
        "--json",
    ]));
    assert_eq!(
        plan["steps"][0]["points"], cli["steps"][0]["points"],
        "mcp: {plan}\ncli: {cli}"
    );
}

/// Every tool the server advertises must be callable. A schema for a tool
/// that errors on invocation is worse than no tool.
#[test]
fn the_mcp_server_advertises_only_tools_it_has() {
    scenario!(_f);
    let initialize = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2026-07-28", "capabilities": {},
                    "clientInfo": { "name": "scenarios", "version": "0" } },
    });
    let list = serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    let out = speak(&["mcp"], &format!("{initialize}\n{list}\n"));
    let replies = jsonrpc_replies(&out);

    let listed = replies
        .iter()
        .find(|r| r["id"] == 2)
        .unwrap_or_else(|| panic!("no tools/list reply: {replies:?}"));
    let tools = listed["result"]["tools"].as_array().expect("a tool array");
    assert!(!tools.is_empty(), "an MCP server with no tools: {listed}");
    for tool in tools {
        assert!(
            tool["name"].as_str().is_some_and(|n| !n.is_empty()),
            "a tool without a name: {tool}"
        );
        assert!(
            tool["inputSchema"].is_object(),
            "a tool without a schema: {tool}"
        );
        assert!(
            tool["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("pixelactions_")),
            "tools are namespaced so a model can tell whose they are: {tool}"
        );
    }
}

/// A malformed question is a protocol error; a negative answer is not. A
/// model that reads a refusal as a broken tool retries instead of reacting,
/// which is the failure this distinction exists to prevent.
#[test]
fn mcp_reports_an_unknown_tool_as_an_error() {
    scenario!(_f);
    let initialize = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2026-07-28", "capabilities": {},
                    "clientInfo": { "name": "scenarios", "version": "0" } },
    });
    let call = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "pixelactions_no_such_tool", "arguments": {} },
    });
    let out = speak(&["mcp"], &format!("{initialize}\n{call}\n"));
    let replies = jsonrpc_replies(&out);
    let reply = replies
        .iter()
        .find(|r| r["id"] == 2)
        .unwrap_or_else(|| panic!("no reply: {replies:?}"));
    assert!(
        reply.get("error").is_some() || reply["result"]["isError"] == true,
        "an unknown tool must not look like a successful call: {reply}"
    );
}

/// Feed a surface some stdin and collect what it says.
fn speak(args: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut child = std::process::Command::new(binary())
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the surface starts");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("written");
    child.wait_with_output().expect("it finishes")
}

/// The JSON-RPC replies on stdout, ignoring anything that is not one.
fn jsonrpc_replies(out: &Output) -> Vec<serde_json::Value> {
    stdout(out)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v.get("id").is_some())
        .collect()
}

// ---------------------------------------------------------------------------
// The flags and tools the first audit missed
// ---------------------------------------------------------------------------

/// `--space` overrides the flow's coordinate space, and the answer has to
/// say which space it is in — a number with the wrong space is a click in
/// the wrong place on a scaled display.
///
/// The two spaces are checked against each other rather than against fixed
/// numbers: physical is logical times the monitor's scale. That holds on a
/// Retina macOS runner where they differ and on Windows and Linux where
/// they do not, so the test means something everywhere instead of passing
/// vacuously on two platforms out of three.
#[test]
fn every_coordinate_space_is_reported_as_the_one_that_was_asked_for() {
    scenario!(f);
    let point = |space: &str| -> serde_json::Value {
        let out = run(&[
            "plan",
            "--session",
            &f.path(),
            "click:target",
            "--space",
            space,
            "--json",
        ]);
        assert_eq!(code(&out), 0, "--space {space}: {}", stdout(&out));
        json(&out)["steps"][0]["points"][0].clone()
    };

    let physical = point("physical");
    let logical = point("logical");
    let auto = point("auto");

    assert_eq!(physical["space"], "physical", "{physical}");
    assert_eq!(logical["space"], "logical", "{logical}");

    let scale = logical["scale"].as_f64().unwrap_or(1.0);
    let (px, lx) = (
        physical["x"].as_f64().expect("an x"),
        logical["x"].as_f64().expect("an x"),
    );
    assert!(
        (px - lx * scale).abs() <= 1.0,
        "physical {px} should be logical {lx} times the scale {scale}"
    );

    // `auto` is whichever of the two this platform's input API wants, so it
    // must be one of them and not a third answer.
    assert!(
        auto["x"] == physical["x"] || auto["x"] == logical["x"],
        "auto resolved to neither space: auto={auto} physical={physical} logical={logical}"
    );
}

/// `doctor --probe` moves the cursor a pixel and puts it back, to prove
/// input permission rather than assume it.
///
/// A runner may or may not grant that — macOS will not without TCC — so
/// this does not assert it succeeds. It asserts the report is *honest*:
/// the probe says it was attempted, and never claims to have confirmed
/// something it did not observe move. A probe that reported success
/// without moving anything would be worse than no probe.
#[test]
fn the_probe_reports_what_it_actually_observed() {
    if !enabled() {
        return;
    }
    let out = run(&["doctor", "--probe", "--json"]);
    let report = json(&out);
    let probe = &report["probe"];

    assert_eq!(
        probe["attempted"], true,
        "--probe must record that it tried: {report}"
    );
    if probe["confirmed"] == true {
        assert_eq!(
            probe["moved"], true,
            "a probe cannot confirm a move it did not see: {report}"
        );
    }
}

/// Without `--probe`, nothing is attempted — the check is opt-in because
/// it posts a real event, and a `doctor` that moved the cursor unasked
/// would be a surprise.
#[test]
fn without_the_probe_flag_nothing_is_posted() {
    if !enabled() {
        return;
    }
    let report = json(&run(&["doctor", "--json"]));
    assert_eq!(
        report["probe"]["attempted"], false,
        "doctor posted an event nobody asked for: {report}"
    );
}

/// `pixelactions_act` on a server launched without `--yes` must come back
/// as an ordinary *refusal*, not a protocol error.
///
/// This is the whole reason the distinction exists: a model that reads a
/// refusal as a broken tool retries, and a retrying model that eventually
/// gets through is exactly the runaway this gate exists to prevent. So
/// `isError` stays false, `ok` is false, and the text says what a human
/// would have to do.
#[test]
fn acting_without_consent_is_a_refusal_not_an_error() {
    scenario!(f);
    let initialize = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2026-07-28", "capabilities": {},
                    "clientInfo": { "name": "scenarios", "version": "0" } },
    });
    let call = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "pixelactions_act",
                    "arguments": { "session": f.path(), "steps": ["click:target"] } },
    });
    let out = speak(
        &["mcp"],
        &format!(
            "{initialize}
{call}
"
        ),
    );
    let replies = jsonrpc_replies(&out);
    let reply = replies
        .iter()
        .find(|r| r["id"] == 2)
        .unwrap_or_else(|| panic!("no reply: {replies:?}"));

    assert_eq!(
        reply["result"]["isError"],
        serde_json::Value::Bool(false),
        "a refusal is an answer, not a broken tool: {reply}"
    );
    assert_eq!(
        reply["result"]["structuredContent"]["ok"],
        serde_json::Value::Bool(false),
        "{reply}"
    );
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        text.contains("--yes"),
        "the refusal must say what would allow it: {text}"
    );
}

/// `pixelactions_find` asks pixelcoords where the regions are now. It is
/// the read-only half of the MCP surface and is always safe, so it works
/// with or without `--yes`.
#[test]
fn the_find_tool_locates_the_region() {
    scenario!(f);
    if !markable(&f) {
        return;
    }
    let initialize = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2026-07-28", "capabilities": {},
                    "clientInfo": { "name": "scenarios", "version": "0" } },
    });
    let call = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "pixelactions_find",
                    "arguments": { "session": f.path() } },
    });
    let out = speak(
        &["mcp"],
        &format!(
            "{initialize}
{call}
"
        ),
    );
    let replies = jsonrpc_replies(&out);
    let reply = replies
        .iter()
        .find(|r| r["id"] == 2)
        .unwrap_or_else(|| panic!("no reply: {replies:?}"));

    assert_eq!(
        reply["result"]["isError"],
        serde_json::Value::Bool(false),
        "{reply}"
    );
    assert_eq!(
        reply["result"]["structuredContent"]["ok"],
        serde_json::Value::Bool(true),
        "the region is on screen, so find should say so: {reply}"
    );
}
