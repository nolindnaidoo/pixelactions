//! The line protocol: one JSON object per line, in and out.
//!
//! This is what makes a bot in *any* language possible without this
//! crate knowing anything about that language. Every language can spawn
//! a process and write lines to a pipe; none of them need FFI, native
//! modules, or a package we have to release in lockstep.
//!
//! The framing is the one LSP, esbuild, and MCP all converged on:
//!
//! - one JSON object per line, no embedded newlines
//! - **stdout carries only protocol messages; stderr is for logs** — a
//!   client must never read stderr output as failure
//! - closing stdin is the graceful shutdown; there is no daemon, no PID
//!   file, and no lifetime for anyone to manage
//! - a version handshake on the first exchange, so changing the protocol
//!   later does not break every existing bot (ripgrep's `--json` shipped
//!   without one, and that is the cautionary tale)
//!
//! Requests carry an `id` that responses echo. Only one request is in
//! flight at a time today — that keeps the implementation free of an
//! async runtime — but the `id` means concurrency can arrive later
//! without a breaking change.

use serde::{Deserialize, Serialize};

use crate::convert::ResolvedPoint;
use crate::flow::{Settings, Step};

/// The protocol version this build speaks. Bumped only for changes a
/// client could notice.
pub const PROTOCOL_VERSION: u32 = 1;

/// A request from the client.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Echoed on the response. Any value the client finds useful.
    pub id: Option<u64>,
    pub body: RequestBody,
}

/// What the client wants done.
///
/// `do` names either a control message or a step, using the same
/// vocabulary as flow files and chained argv — so `{"do":"click"}` is
/// the wire form of `action = "click"`. Because one key names both
/// kinds, the two are separated by hand in [`parse_request`] rather
/// than by a derive; the payoff is that a client writes the verb it
/// already knows instead of nesting it under a wrapper.
#[derive(Debug, Clone, PartialEq)]
pub enum RequestBody {
    /// First message: agree on a version before anything else happens,
    /// and optionally set the run settings for the whole session — the
    /// same fields a flow file's `[settings]` table carries, so there is
    /// nothing new to learn and no CLI flag to keep in sync.
    Hello {
        version: u32,
        settings: Option<Settings>,
    },
    /// Perform one step.
    Step { step: Step },
    /// Re-locate every region and report where they are, without acting.
    Relocate,
    /// End the session. Closing stdin does the same thing.
    Bye,
}

/// A response to the client. Always exactly one per request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub body: ResponseBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ResponseBody {
    /// Answer to `hello`: what this build speaks and what it can do.
    Welcome {
        version: u32,
        /// Step names this build understands, so a client can degrade
        /// gracefully instead of guessing.
        verbs: Vec<String>,
        session: String,
    },
    /// A step ran. `outcome` is the same vocabulary a run report uses:
    /// `verified` · `executed` · `failed`.
    ///
    /// A step that ran and failed honestly is a `done` with
    /// `outcome: "failed"` and a `detail`; a request that could not be
    /// understood or resolved is an `error`. That is the same line the
    /// exit codes draw between 1 and 2.
    Done {
        outcome: String,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        points: Vec<ResolvedPoint>,
        /// Why it failed, in words — including the evidence a timeout
        /// gathered. Absent when nothing went wrong.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        detail: Option<String>,
        elapsed_ms: u64,
    },
    /// Regions were re-located.
    Located {
        /// Labels whose current position differs from the session.
        moved: Vec<String>,
        /// Labels that could not be found unambiguously — acting on
        /// these would be acting blind.
        missing: Vec<String>,
    },
    /// Acknowledgment of `bye`. The last line before stdout closes.
    Closed,
    /// The request could not be honored. `detail` is written for a human
    /// to read and act on, not for a stack trace.
    Error { detail: String },
}

impl Response {
    pub fn error(id: Option<u64>, detail: impl Into<String>) -> Self {
        Self {
            id,
            body: ResponseBody::Error {
                detail: detail.into(),
            },
        }
    }

    /// Serialize to a single line, newline included.
    ///
    /// Serialization cannot fail for these types, but a client would be
    /// left hanging if it ever did — so a failure becomes a protocol
    /// error rather than a panic or a silent drop.
    pub fn to_line(&self) -> String {
        match serde_json::to_string(self) {
            Ok(json) => format!("{json}\n"),
            Err(error) => format!(
                "{{\"result\":\"error\",\"detail\":\"could not serialize response: {error}\"}}\n"
            ),
        }
    }
}

/// Best-effort read of a line's `id`, for correlating an error with the
/// request that caused it.
///
/// [`parse_request`] fails before it can build a `Request`, which would
/// otherwise leave a client with several requests outstanding unable to
/// tell which one was rejected. A line too broken to yield an `id` gets
/// `None` — the one case where a client has to fall back to order.
pub fn id_in(line: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()?
        .get("id")?
        .as_u64()
}

/// Parse one line into a request.
///
/// Control verbs are recognized first; anything else is handed to the
/// step deserializer under its own tag, so step shapes stay defined in
/// exactly one place (`flow::Step`) and can never drift from the flow
/// file format.
pub fn parse_request(line: &str) -> Result<Request, String> {
    let mut value: serde_json::Value = serde_json::from_str(line).map_err(|error| {
        format!(
            "not a valid request: {error}. \
             Expected one JSON object per line, e.g. \
             {{\"id\":1,\"do\":\"click\",\"target\":\"submit\"}}"
        )
    })?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "a request must be a JSON object".to_string())?;

    let id = object.get("id").and_then(serde_json::Value::as_u64);
    let verb = object
        .remove("do")
        .ok_or_else(|| "a request needs a \"do\" naming the action, e.g. \"click\"".to_string())?;
    let verb = verb
        .as_str()
        .ok_or_else(|| "\"do\" must be a string".to_string())?
        .to_string();

    let body = match verb.as_str() {
        "hello" => {
            let version = object
                .get("version")
                .and_then(serde_json::Value::as_u64)
                .and_then(|version| u32::try_from(version).ok())
                .ok_or_else(|| {
                    format!(
                        "hello needs a \"version\" number; \
                         this build speaks version {PROTOCOL_VERSION}"
                    )
                })?;
            let settings = match object.remove("settings") {
                None => None,
                Some(value) => {
                    Some(serde_json::from_value::<Settings>(value).map_err(|error| {
                        format!("hello carried settings this build cannot read: {error}")
                    })?)
                }
            };
            RequestBody::Hello { version, settings }
        }
        "relocate" => RequestBody::Relocate,
        "bye" => RequestBody::Bye,
        step_name => {
            object.remove("id");
            object.insert(
                "action".to_string(),
                serde_json::Value::String(step_name.to_string()),
            );
            // serde already names every known action on an unknown
            // variant, and names the missing key on a malformed one —
            // appending our own list would only say it twice.
            let step: Step = serde_json::from_value(value)
                .map_err(|error| format!("cannot read {step_name:?} as an action: {error}"))?;
            RequestBody::Step { step }
        }
    };
    Ok(Request { id, body })
}

/// Every step name this build understands — the handshake's answer to
/// "what can you do".
pub fn supported_verbs() -> Vec<String> {
    [
        "click",
        "double_click",
        "drag",
        "type",
        "key",
        "verify",
        "wait_for",
        "wait_gone",
        "pause",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_request_reads_the_same_vocabulary_as_flows() {
        let request = parse_request(r#"{"id":7,"do":"click","target":"submit"}"#).expect("valid");
        assert_eq!(request.id, Some(7));
        assert_eq!(
            request.body,
            RequestBody::Step {
                step: Step::Click {
                    target: "submit".into()
                }
            }
        );
    }

    #[test]
    fn every_step_kind_crosses_the_wire() {
        for (line, expected) in [
            (
                r#"{"do":"type","text":"hello"}"#,
                Step::Type {
                    text: "hello".into(),
                },
            ),
            (
                r#"{"do":"key","chord":"cmd+s"}"#,
                Step::Key {
                    chord: "cmd+s".into(),
                },
            ),
            (r#"{"do":"pause","ms":250}"#, Step::Pause { ms: 250 }),
            (
                r#"{"do":"drag","from":"a","to":"b"}"#,
                Step::Drag {
                    from: "a".into(),
                    to: "b".into(),
                },
            ),
            (
                r#"{"do":"wait_for","target":"dialog"}"#,
                Step::WaitFor {
                    target: "dialog".into(),
                },
            ),
        ] {
            let request = parse_request(line).expect("valid");
            assert_eq!(
                request.body,
                RequestBody::Step { step: expected },
                "line: {line}"
            );
        }
    }

    #[test]
    fn an_id_is_optional_and_echoed_when_present() {
        assert_eq!(
            parse_request(r#"{"do":"relocate"}"#).expect("valid").id,
            None
        );
        let response = Response::error(Some(3), "nope");
        assert!(response.to_line().contains("\"id\":3"));
    }

    #[test]
    fn a_response_is_exactly_one_line() {
        let response = Response {
            id: Some(1),
            body: ResponseBody::Done {
                outcome: "verified".into(),
                points: Vec::new(),
                detail: None,
                elapsed_ms: 12,
            },
        };
        let line = response.to_line();
        assert_eq!(line.matches('\n').count(), 1, "exactly one newline");
        assert!(line.ends_with('\n'));
        assert!(!line.trim_end().contains('\n'), "no embedded newlines");
    }

    #[test]
    fn garbage_becomes_a_readable_error_not_a_crash() {
        let error = parse_request("not json at all").expect_err("should fail");
        assert!(
            error.contains("one JSON object per line"),
            "teaches the shape: {error}"
        );
    }

    #[test]
    fn an_unknown_verb_is_refused_rather_than_guessed() {
        assert!(parse_request(r#"{"do":"teleport","target":"home"}"#).is_err());
    }

    #[test]
    fn a_request_without_a_verb_says_so() {
        let error = parse_request(r#"{"id":1}"#).expect_err("no verb");
        assert!(error.contains("\"do\""), "names the missing key: {error}");
    }

    #[test]
    fn a_malformed_step_names_the_field_it_wanted() {
        // Right verb, wrong shape: click needs a target.
        let error = parse_request(r#"{"do":"click"}"#).expect_err("missing target");
        assert!(error.contains("target"), "names the missing field: {error}");
    }

    #[test]
    fn an_unknown_verb_lists_the_ones_that_exist() {
        let error = parse_request(r#"{"do":"teleport"}"#).expect_err("unknown");
        assert!(error.contains("wait_for"), "lists what exists: {error}");
    }

    #[test]
    fn the_handshake_advertises_what_this_build_can_do() {
        let request = parse_request(r#"{"id":0,"do":"hello","version":1}"#).expect("valid");
        assert_eq!(
            request.body,
            RequestBody::Hello {
                version: PROTOCOL_VERSION,
                settings: None
            }
        );
        let verbs = supported_verbs();
        assert!(verbs.contains(&"click".to_string()));
        assert!(verbs.contains(&"wait_for".to_string()));
    }

    #[test]
    fn a_handshake_can_carry_run_settings() {
        let request =
            parse_request(r#"{"do":"hello","version":1,"settings":{"timeout_ms":30000}}"#)
                .expect("valid");
        let RequestBody::Hello { settings, .. } = request.body else {
            panic!("expected a hello");
        };
        let settings = settings.expect("settings were sent");
        assert_eq!(settings.timeout_ms, 30_000);
        // Unnamed fields keep their defaults rather than becoming zero.
        assert!(settings.relocate);
    }

    #[test]
    fn a_typo_in_settings_is_an_error_not_a_silent_default() {
        // Our own config is parsed strictly — see AGENTS.md.
        let error = parse_request(r#"{"do":"hello","version":1,"settings":{"timeout":1}}"#)
            .expect_err("unknown field");
        assert!(error.contains("settings"), "says where: {error}");
    }

    #[test]
    fn a_failed_step_carries_its_reason() {
        let response = Response {
            id: None,
            body: ResponseBody::Done {
                outcome: "failed".into(),
                points: Vec::new(),
                detail: Some("timed out after 10000ms".into()),
                elapsed_ms: 10_004,
            },
        };
        assert!(response.to_line().contains("timed out"));
    }

    #[test]
    fn a_rejected_request_can_still_be_matched_to_its_id() {
        // The step is malformed, so parsing fails — but a client with
        // several requests in flight still needs to know which one.
        assert_eq!(id_in(r#"{"id":42,"do":"click"}"#), Some(42));
        assert_eq!(id_in(r#"{"do":"click"}"#), None);
        assert_eq!(id_in("not json"), None);
    }

    #[test]
    fn responses_round_trip() {
        let response = Response {
            id: Some(9),
            body: ResponseBody::Located {
                moved: vec!["submit".into()],
                missing: Vec::new(),
            },
        };
        let line = response.to_line();
        let back: Response = serde_json::from_str(line.trim_end()).expect("round trip");
        assert_eq!(back, response);
    }
}
