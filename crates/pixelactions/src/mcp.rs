//! The executor half of the loop, reachable by a model over stdio.
//!
//! pixelcoords serves a read-only MCP server: a model can ask it *where*
//! to click, in microseconds, sending no image. Nothing let it say *do
//! it*. This is that half.
//!
//! # Why acting needs a gate, and where the gate lives
//!
//! pixelcoords' server is read-only, so it has a safe default. This one
//! posts real input, so it has none. `--yes` is the gate on the CLI and a
//! model cannot pass a CLI flag.
//!
//! So the gate is on the **server**: `pixelactions mcp` serves read-only,
//! `pixelactions mcp --yes` also allows acting. The consent then lives
//! with the person who edited the client's config — somewhere the model
//! cannot reach — which is the same shape `--yes` already has, moved to
//! the only place a stdio server has a human in it. A per-call `confirm`
//! argument would be written by the model itself: a speed bump against a
//! slip, not a gate against intent.
//!
//! `pixelactions_act` is still **advertised** without `--yes`, and
//! refuses. Hiding it would make a model conclude the tool does not
//! exist; refusing with a message it can relay lets it tell the user what
//! to change.
//!
//! # The rule this shares with the sister tool
//!
//! **A refusal is an answer, not an error.** A step that failed, a region
//! that could not be found, an act call on a read-only server — all come
//! back as ordinary results with `ok: false`, never as a protocol error.
//! A model that reads a refusal as *the tool is broken* retries instead of
//! reacting, and for a tool that injects input that is worse than not
//! serving it at all. Only a malformed question is an error.
//!
//! # One implementation, four surfaces
//!
//! `AGENTS.md`: chained verbs, flow files and `serve` all build the same
//! `Flow` and go through the same `plan` → `run::execute` path, and a
//! surface that grows its own copy of relocation, the kill switch or
//! verification is a bug. This is the fourth, and it adds nothing
//! underneath — a model-driven run gets the kill switch and the audit log
//! because they were already there.

use std::io::{BufRead, Write};

use anyhow::Result;
use serde_json::{Map, Value, json};

use crate::{Source, audit, doctor, inject, run, verify};

/// Revisions this server speaks, newest first. Matches pixelcoords, so a
/// client driving both halves of the loop negotiates once.
const PROTOCOL_VERSIONS: [&str; 2] = ["2026-07-28", "2025-11-25"];

const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

/// Renumbered from -32004 in this revision.
const ERR_UNSUPPORTED_PROTOCOL: i64 = -32022;
const ERR_PARSE: i64 = -32700;
const ERR_INVALID_REQUEST: i64 = -32600;
const ERR_METHOD_NOT_FOUND: i64 = -32601;
const ERR_INVALID_PARAMS: i64 = -32602;

/// The tool list is static, so a client may cache it for an hour.
const TOOLS_TTL_MS: u64 = 3_600_000;

/// Tools, in the order a model should reach for them: look, then act.
const TOOLS: [&str; 3] = ["pixelactions_plan", "pixelactions_act", "pixelactions_find"];

/// Serve on stdio until stdin closes.
///
/// `allow_acting` is `--yes`. Everything else about the server is the
/// same either way, so the flag reaches exactly one decision.
pub fn serve(allow_acting: bool) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Some(response) = handle_line(allow_acting, &line) else {
            continue;
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}

/// One line in, at most one line out.
///
/// Split from [`serve`] because it is pure dispatch — JSON in, JSON out —
/// so everything except the acting path itself tests without a window
/// system or a pipe.
fn handle_line(allow_acting: bool, line: &str) -> Option<String> {
    let request: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(e) => return Some(error_response(&Value::Null, ERR_PARSE, &format!("{e}"))),
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if id.is_null() {
        // A notification: no id, so no reply.
        return None;
    }
    if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(error_response(
            &id,
            ERR_INVALID_REQUEST,
            "every request needs \"jsonrpc\": \"2.0\"",
        ));
    }
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return Some(error_response(&id, ERR_INVALID_REQUEST, "no method"));
    };
    let params = request.get("params");

    if let Some(version) = params
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(META_PROTOCOL_VERSION))
        .and_then(Value::as_str)
        && !PROTOCOL_VERSIONS.contains(&version)
    {
        return Some(error_response(
            &id,
            ERR_UNSUPPORTED_PROTOCOL,
            &format!(
                "protocol version {version} is not supported; this server speaks {}",
                PROTOCOL_VERSIONS.join(", ")
            ),
        ));
    }

    let outcome = match method {
        "server/discover" => Ok(discover()),
        // Older clients open with this. A static document, and it buys
        // every client shipping today.
        "initialize" => Ok(initialize()),
        "tools/list" => Ok(tools_list(allow_acting)),
        "tools/call" => call_tool(allow_acting, params),
        other => {
            return Some(error_response(
                &id,
                ERR_METHOD_NOT_FOUND,
                &format!("unknown method {other}"),
            ));
        }
    };
    Some(match outcome {
        Ok(result) => success_response(&id, result),
        Err(message) => error_response(&id, ERR_INVALID_PARAMS, &message),
    })
}

fn server_info() -> Value {
    json!({
        "name": "pixelactions",
        "title": "pixelactions",
        "version": env!("CARGO_PKG_VERSION"),
    })
}

fn success_response(id: &Value, mut result: Value) -> String {
    if let Some(object) = result.as_object_mut() {
        object.insert("resultType".into(), json!("complete"));
        let meta = object
            .entry("_meta")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(meta) = meta.as_object_mut() {
            meta.insert(META_SERVER_INFO.into(), server_info());
        }
    }
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_response(id: &Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

fn capabilities() -> Value {
    json!({ "tools": {} })
}

fn discover() -> Value {
    json!({
        "protocolVersions": PROTOCOL_VERSIONS,
        "capabilities": capabilities(),
        "serverInfo": server_info(),
    })
}

fn initialize() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSIONS[0],
        "capabilities": capabilities(),
        "serverInfo": server_info(),
    })
}

fn tools_list(allow_acting: bool) -> Value {
    json!({
        "tools": TOOLS.iter().map(|name| tool_schema(name, allow_acting)).collect::<Vec<_>>(),
        "ttlMs": TOOLS_TTL_MS,
        "cacheScope": "public",
    })
}

/// The steps argument, shared by `plan` and `act`.
fn steps_schema() -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" },
        "minItems": 1,
        "description":
            "Steps in verb:argument form, in order: click:submit, double:icon, \
             type:hello, key:cmd+s, drag:card>bin, scroll:results>3, \
             hscroll:table>2, verify:banner, changed:panel, changed:panel>2.5, \
             wait:dialog, gone:spinner, pause:250. A label is a region a human \
             marked in pixelcoords; ask pixelcoords where they are.",
    })
}

fn tool_schema(name: &str, allow_acting: bool) -> Value {
    match name {
        "pixelactions_plan" => json!({
            "name": "pixelactions_plan",
            "description":
                "Resolve steps against a session and return every coordinate that \
                 would be acted on, in the units this platform's input API expects. \
                 Touches nothing — no clicks, no typing, no capture. Reach for this \
                 before pixelactions_act to see what would happen, and to check a \
                 label exists.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session": { "type": "string", "description": "Path to a pixelcoords session directory." },
                    "steps": steps_schema(),
                },
                "required": ["session", "steps"],
                "additionalProperties": false,
            },
        }),
        "pixelactions_act" => json!({
            "name": "pixelactions_act",
            "description": act_description(allow_acting),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session": { "type": "string", "description": "Path to a pixelcoords session directory." },
                    "steps": steps_schema(),
                },
                "required": ["session", "steps"],
                "additionalProperties": false,
            },
        }),
        _ => json!({
            "name": "pixelactions_find",
            "description":
                "Re-locate a session's regions in a fresh capture and report where \
                 they are now, with the offset from where they were marked. Use it \
                 when a step refused because a region moved. Captures the screen; \
                 pixelactions_plan does not.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session": { "type": "string", "description": "Path to a pixelcoords session directory." },
                    "label": { "type": "string", "description": "Only this region. Omit for every region." },
                },
                "required": ["session"],
                "additionalProperties": false,
            },
        }),
    }
}

/// The description changes with the gate, so a model reading the tool list
/// on a read-only server learns *why* before it spends a call.
fn act_description(allow_acting: bool) -> String {
    let base = "Perform steps against a session: move the pointer to a marked region \
                and click, type, drag or scroll. Every acting step confirms the region \
                is still there before touching it, and the run stops rather than \
                clicking a region that moved somewhere ambiguous. This posts real \
                input to the machine.";
    if allow_acting {
        return base.to_string();
    }
    format!(
        "{base} DISABLED on this server: it was launched without --yes, so this tool \
         refuses every call. Tell the user to relaunch it as `pixelactions mcp --yes` \
         if they want it to act; you cannot enable it yourself."
    )
}

fn call_tool(allow_acting: bool, params: Option<&Value>) -> Result<Value, String> {
    let params = params.ok_or("tools/call needs params")?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("tools/call needs a tool name")?;
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    if !TOOLS.contains(&name) {
        return Err(format!(
            "unknown tool {name}; this server serves {}",
            TOOLS.join(", ")
        ));
    }
    let object = arguments
        .as_object()
        .ok_or("arguments must be an object")?
        .clone();

    match name {
        "pixelactions_plan" => tool_plan(&object),
        "pixelactions_act" => tool_act(allow_acting, &object),
        _ => tool_find(&object),
    }
}

/// Reject an argument this tool does not take, naming the ones it does.
fn only(object: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!(
                "unknown argument {key}; this tool takes {}",
                allowed.join(", ")
            ));
        }
    }
    Ok(())
}

fn session_of(object: &Map<String, Value>) -> Result<std::path::PathBuf, String> {
    object
        .get("session")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "session is required, and must be a path".to_string())
}

fn steps_of(object: &Map<String, Value>) -> Result<Vec<String>, String> {
    let steps = object
        .get("steps")
        .and_then(Value::as_array)
        .ok_or("steps is required, and must be an array of verb:argument strings")?;
    if steps.is_empty() {
        return Err("steps must not be empty".to_string());
    }
    steps
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "every step must be a string, e.g. click:submit".to_string())
        })
        .collect()
}

/// Stamp the aggregate every tool's answer carries.
///
/// The whole contract is "read `ok`, not `isError`", which only works if
/// every tool actually has one.
fn with_ok(structured: &mut Value, ok: bool) {
    if let Some(object) = structured.as_object_mut() {
        object.insert("ok".into(), json!(ok));
    }
}

/// A result a model should react to. `isError` stays false — see the
/// module docs.
fn report_result(structured: &Value, summary: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": summary }],
        "structuredContent": structured,
        "isError": false,
    })
}

fn tool_plan(object: &Map<String, Value>) -> Result<Value, String> {
    only(object, &["session", "steps"])?;
    let source = Source {
        flow: None,
        session: Some(session_of(object)?),
        verbs: steps_of(object)?,
    };
    let (plan, mut structured) = crate::plan_source(&source).map_err(|e| format!("{e:#}"))?;
    // Every tool answers with a top-level `ok`, because that is the field
    // the caller is told to read. A plan that returned at all resolved.
    with_ok(&mut structured, true);
    let summary = format!("{} step(s) resolved; nothing was touched", plan.steps.len());
    Ok(report_result(&structured, &summary))
}

fn tool_act(allow_acting: bool, object: &Map<String, Value>) -> Result<Value, String> {
    only(object, &["session", "steps"])?;
    if !allow_acting {
        // A refusal, not an error: the question was well formed and this
        // is its answer. A model that sees an error retries; one that
        // sees `ok: false` with a reason can relay it.
        let structured = json!({
            "ok": false,
            "refused": "acting is disabled on this server",
            "remedy": "relaunch it as `pixelactions mcp --yes`",
        });
        return Ok(report_result(
            &structured,
            "refused: this server was launched without --yes, so it will not post input. \
             Ask the user to relaunch it as `pixelactions mcp --yes`.",
        ));
    }
    let source = Source {
        flow: None,
        session: Some(session_of(object)?),
        verbs: steps_of(object)?,
    };
    let report = crate::act_source(&source).map_err(|e| format!("{e:#}"))?;
    let mut structured = serde_json::to_value(&report).map_err(|e| e.to_string())?;
    let ok = report.exit_code() == 0;
    // `RunReport` carries per-step outcomes and an exit code, not an
    // aggregate. A model is told to read `ok`, so `ok` has to be there —
    // and it must mean what the exit code means, not something adjacent.
    with_ok(&mut structured, ok);
    let summary = if ok {
        format!("{} step(s) performed", report.steps.len())
    } else {
        let failed = report
            .steps
            .iter()
            .find(|s| s.detail.is_some())
            .and_then(|s| s.detail.clone())
            .unwrap_or_else(|| "a step did not succeed".to_string());
        format!("run stopped: {failed}")
    };
    Ok(report_result(&structured, &summary))
}

fn tool_find(object: &Map<String, Value>) -> Result<Value, String> {
    only(object, &["session", "label"])?;
    let session = session_of(object)?;
    let label = object.get("label").and_then(Value::as_str);
    let report = verify::find(&session, label).map_err(|e| format!("{e:#}"))?;
    // Found **and unambiguous** — the rule `is_confirmed` states and the
    // acting path enforces. Counting `found` alone would tell a model a
    // region matching in three places is located, and then refuse the act
    // call that followed, which is the contradiction `ok` exists to
    // prevent.
    let (confirmed, ambiguous) = report.tally();
    // When a region is ambiguous, say why: the bare count reads like a
    // plain miss, and a model would have no idea the crop is the problem
    // rather than the screen.
    let note = if ambiguous > 0 {
        format!(
            " ({ambiguous} matched in more than one place, so there is no point worth acting on)"
        )
    } else {
        String::new()
    };
    let summary = format!(
        "{confirmed}/{} region(s) located{note}",
        report.results.len()
    );
    let structured = json!({
        "ok": report.all_confirmed(),
        "results": report.results.iter().map(|r| json!({
            "label": r.label,
            "found": r.found,
            "ambiguous": r.ambiguous,
            "score": r.score,
            "monitor": r.monitor,
            "delta": r.delta.map(|d| json!({ "dx": d.dx, "dy": d.dy })),
        })).collect::<Vec<_>>(),
    });
    Ok(report_result(&structured, &summary))
}

/// Refuse before serving anything if this machine cannot act at all.
///
/// Reported once at startup rather than per call: a model that gets the
/// same refusal three times learns nothing the first one did not say, and
/// the operator is the one who has to fix it.
pub fn preflight(allow_acting: bool) -> Option<String> {
    if let Err(reason) = doctor::require_supported_pixelcoords() {
        return Some(reason);
    }
    if allow_acting && let Err(reason) = inject::availability() {
        return Some(reason);
    }
    let _ = audit::log_path();
    let _ = run::no_audit();
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(request: &Value, allow_acting: bool) -> Value {
        let line = handle_line(allow_acting, &request.to_string()).expect("a reply");
        serde_json::from_str(&line).expect("valid JSON")
    }

    fn result(request: &Value, allow_acting: bool) -> Value {
        let mut reply = ask(request, allow_acting);
        reply["result"].take()
    }

    fn error_code(request: &Value, allow_acting: bool) -> i64 {
        ask(request, allow_acting)["error"]["code"]
            .as_i64()
            .expect("an error code")
    }

    fn call(tool: &str, arguments: &Value, allow_acting: bool) -> Value {
        result(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                    "params":{"name":tool,"arguments":arguments}}),
            allow_acting,
        )
    }

    #[test]
    fn discover_advertises_the_versions_and_identity() {
        let r = result(
            &json!({"jsonrpc":"2.0","id":1,"method":"server/discover"}),
            false,
        );
        assert_eq!(r["protocolVersions"][0], "2026-07-28");
        assert_eq!(r["serverInfo"]["name"], "pixelactions");
        assert_eq!(r["resultType"], "complete");
    }

    #[test]
    fn older_clients_still_get_an_initialize_answer() {
        let r = result(
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
            false,
        );
        assert_eq!(r["protocolVersion"], "2026-07-28");
    }

    #[test]
    fn three_tools_in_a_deterministic_order() {
        let r = result(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
            false,
        );
        let names: Vec<&str> = r["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|t| t["name"].as_str().expect("name"))
            .collect();
        assert_eq!(
            names,
            ["pixelactions_plan", "pixelactions_act", "pixelactions_find"]
        );
        assert_eq!(r["ttlMs"], 3_600_000);
        assert_eq!(r["cacheScope"], "public");
    }

    /// The gate: the tool is advertised either way, so a model learns it
    /// exists and why it cannot use it — rather than concluding this
    /// server cannot act at all.
    #[test]
    fn act_is_advertised_without_yes_and_says_it_is_disabled() {
        let r = result(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
            false,
        );
        let act = r["tools"][1]["description"].as_str().expect("description");
        assert!(act.contains("DISABLED"), "{act}");
        assert!(act.contains("--yes"), "names the remedy: {act}");

        let r = result(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}), true);
        let act = r["tools"][1]["description"].as_str().expect("description");
        assert!(!act.contains("DISABLED"), "{act}");
    }

    /// **The rule that matters.** A refused act is an answer, not an
    /// error: a model that sees a protocol error retries, and retrying a
    /// tool that posts input is the worst failure this server could have.
    #[test]
    fn a_refused_act_is_an_answer_not_an_error() {
        let r = call(
            "pixelactions_act",
            &json!({"session": "/tmp/nope", "steps": ["click:submit"]}),
            false,
        );
        assert_eq!(r["isError"], false, "must not look like a broken tool");
        assert_eq!(r["structuredContent"]["ok"], false);
        let text = r["content"][0]["text"].as_str().expect("text");
        assert!(text.contains("--yes"), "tells the model the remedy: {text}");
    }

    /// Refusing must not depend on the session being readable — the gate
    /// is checked before anything touches the disk, so a model gets the
    /// real reason rather than a file error.
    #[test]
    fn the_gate_is_checked_before_the_session_is_read() {
        let r = call(
            "pixelactions_act",
            &json!({"session": "/definitely/not/here", "steps": ["click:x"]}),
            false,
        );
        let text = r["content"][0]["text"].as_str().expect("text");
        assert!(text.contains("without --yes"), "{text}");
        assert!(!text.contains("No such file"), "{text}");
    }

    /// Every tool answers with a top-level `ok`. The contract a model is
    /// given is "read ok, not isError", and that only works if it is
    /// always there — including on the tools whose underlying report has
    /// no aggregate of its own.
    #[test]
    fn every_tool_answer_carries_an_ok() {
        let refused = call(
            "pixelactions_act",
            &json!({"session": "/tmp/x", "steps": ["click:a"]}),
            false,
        );
        assert!(refused["structuredContent"]["ok"].is_boolean(), "{refused}");
    }

    #[test]
    fn an_unsupported_protocol_version_is_refused_by_number() {
        let code = error_code(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list",
                    "params":{"_meta":{META_PROTOCOL_VERSION:"1999-01-01"}}}),
            false,
        );
        assert_eq!(code, ERR_UNSUPPORTED_PROTOCOL);
    }

    #[test]
    fn unknown_meta_keys_are_tolerated() {
        let r = result(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list",
                    "params":{"_meta":{"x.vendor/thing":1,
                                       META_PROTOCOL_VERSION:"2026-07-28"}}}),
            false,
        );
        assert!(r["tools"].is_array());
    }

    #[test]
    fn malformed_questions_are_errors() {
        assert_eq!(
            handle_line(false, "not json")
                .and_then(|l| serde_json::from_str::<Value>(&l).ok())
                .expect("reply")["error"]["code"],
            ERR_PARSE
        );
        assert_eq!(
            error_code(&json!({"id":1,"method":"tools/list"}), false),
            ERR_INVALID_REQUEST
        );
        assert_eq!(
            error_code(&json!({"jsonrpc":"2.0","id":1,"method":"nope"}), false),
            ERR_METHOD_NOT_FOUND
        );
        assert_eq!(
            error_code(
                &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                        "params":{"name":"pixelactions_nope","arguments":{}}}),
                false
            ),
            ERR_INVALID_PARAMS
        );
    }

    #[test]
    fn an_unknown_argument_names_the_ones_that_exist() {
        let reply = ask(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                    "params":{"name":"pixelactions_plan",
                              "arguments":{"session":"/tmp/s","steps":["click:x"],"bogus":1}}}),
            false,
        );
        let message = reply["error"]["message"].as_str().expect("message");
        assert!(message.contains("bogus"), "{message}");
        assert!(message.contains("session"), "{message}");
    }

    #[test]
    fn steps_must_be_a_non_empty_array_of_strings() {
        for bad in [
            json!({"session":"/tmp/s"}),
            json!({"session":"/tmp/s","steps":[]}),
            json!({"session":"/tmp/s","steps":[7]}),
        ] {
            let reply = ask(
                &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                        "params":{"name":"pixelactions_plan","arguments":bad}}),
                false,
            );
            assert_eq!(reply["error"]["code"], ERR_INVALID_PARAMS, "{reply}");
        }
    }

    #[test]
    fn a_notification_gets_no_reply() {
        assert!(handle_line(false, &json!({"jsonrpc":"2.0","method":"x"}).to_string()).is_none());
    }
}
