//! Verification — asking the sister tool whether a region is still there.
//!
//! `pixelcoords find` captures the screen fresh and re-locates every
//! selection by its saved crop. That is the only honest way to know an
//! action landed: `assert` never captures, so it can only answer
//! questions about the file.
//!
//! We shell out rather than link, because `find` needs a screen capture —
//! platform work pixelcoords already owns and this crate must not
//! duplicate (see AGENTS.md, the compatibility contract).

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use pixelcoords_core::geometry::Shape;
use serde::Deserialize;

/// The subset of `pixelcoords find --json` this tool reads. Deliberately
/// partial: unknown fields are ignored, so the report can grow upstream
/// without breaking us.
#[derive(Debug, Clone, Deserialize)]
pub struct FindReport {
    pub all_relocated: bool,
    pub results: Vec<FindResult>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FindResult {
    pub label: String,
    pub found: bool,
    #[serde(default)]
    pub ambiguous: bool,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub delta: Option<Delta>,
    #[serde(default)]
    pub reason: Option<String>,
    /// Which monitor the region was re-found on.
    #[serde(default)]
    pub monitor: usize,
    /// Where the region is **now**, in that monitor's local pixels —
    /// parsed as the sister crate's own `Shape` so the corrected click
    /// point comes from its geometry, not a reimplementation here.
    #[serde(default)]
    pub new_px: Option<Shape>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Delta {
    pub dx: i32,
    pub dy: i32,
}

impl FindReport {
    /// The result for one label, matched case-insensitively as pixelcoords
    /// itself matches labels.
    pub fn result_for(&self, label: &str) -> Option<&FindResult> {
        self.results
            .iter()
            .find(|r| r.label.eq_ignore_ascii_case(label))
    }

    /// Whether a label is present, unambiguous, and therefore trustworthy.
    pub fn is_confirmed(&self, label: &str) -> bool {
        self.result_for(label)
            .is_some_and(|r| r.found && !r.ambiguous)
    }

    /// The region's current monitor-local click point, when it was found
    /// unambiguously and reported with geometry.
    ///
    /// `None` means "act on the saved coordinates" — either nothing
    /// moved, or the report carried no new shape.
    pub fn corrected_point(
        &self,
        label: &str,
    ) -> Option<(usize, pixelcoords_core::geometry::Point)> {
        let result = self.result_for(label)?;
        if !result.found || result.ambiguous {
            return None;
        }
        let shape = result.new_px.as_ref()?;
        Some((result.monitor, shape.click_point()))
    }
}

/// Run `pixelcoords find` against a session and parse its report.
///
/// A missing binary, a refusal (changed display), and a genuine
/// not-found are three different things, and the caller needs to tell
/// them apart — so only the first two are errors here.
pub fn find(session: &Path, label: Option<&str>) -> Result<FindReport> {
    let mut command = Command::new("pixelcoords");
    command.arg("find").arg("--session").arg(session);
    if let Some(label) = label {
        command.arg("--label").arg(label);
    }

    let output = command.output().context(
        "cannot run `pixelcoords` — it must be on PATH to relocate or verify \
         (cargo install pixelcoords)",
    )?;

    // Exit 2 means pixelcoords could not answer the question at all
    // (unreadable session, changed display). Exit 0 and 1 both produce a
    // report; 1 just means something was not found.
    if output.status.code() == Some(2) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("pixelcoords find refused: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).context("could not parse the report from pixelcoords find")
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = r#"{
      "schema": 1,
      "captured_utc": "2026-07-28T14:02:11Z",
      "all_relocated": false,
      "results": [
        { "index": 0, "label": "submit", "monitor": 0, "found": true,
          "ambiguous": false, "score": 0.998,
          "old_px": {"x":812,"y":440,"w":96,"h":40},
          "new_px": {"x":812,"y":320,"w":96,"h":40},
          "delta": {"dx":0,"dy":-120} },
        { "index": 1, "label": "gone", "monitor": 0, "found": false,
          "ambiguous": false, "score": 0.42 },
        { "index": 2, "label": "twins", "monitor": 0, "found": true,
          "ambiguous": true, "score": 0.97 }
      ]
    }"#;

    fn report() -> FindReport {
        serde_json::from_str(REPORT).expect("fixture parses")
    }

    #[test]
    fn a_found_unambiguous_region_is_confirmed() {
        assert!(report().is_confirmed("submit"));
    }

    #[test]
    fn a_missing_region_is_not_confirmed() {
        assert!(!report().is_confirmed("gone"));
    }

    #[test]
    fn an_ambiguous_match_is_not_confirmed_even_though_it_was_found() {
        // pixelcoords hands out no coordinates for an ambiguous match, so
        // neither do we — "found twice" is not "found".
        let report = report();
        assert!(report.result_for("twins").expect("present").found);
        assert!(!report.is_confirmed("twins"));
    }

    #[test]
    fn labels_match_case_insensitively() {
        assert!(report().is_confirmed("SUBMIT"));
    }

    #[test]
    fn drift_is_readable_from_the_report() {
        let delta = report()
            .result_for("submit")
            .expect("present")
            .delta
            .expect("moved");
        assert_eq!((delta.dx, delta.dy), (0, -120));
    }

    #[test]
    fn unknown_future_fields_in_the_report_are_ignored() {
        // The compatibility contract, verification side: pixelcoords may
        // add fields to its report at any time.
        let future = REPORT.replace(
            r#""all_relocated": false,"#,
            r#""all_relocated": false, "capture_ms": 680, "engine": "ncc-v2","#,
        );
        let parsed: FindReport = serde_json::from_str(&future).expect("still parses");
        assert!(parsed.is_confirmed("submit"));
    }

    #[test]
    fn a_moved_region_yields_a_corrected_click_point() {
        // submit moved up 120px; its new bbox is 812,320 96x40, whose
        // click point is its center — computed by pixelcoords-core, not
        // by arithmetic written here.
        let (monitor, point) = report()
            .corrected_point("submit")
            .expect("has new geometry");
        assert_eq!(monitor, 0);
        assert_eq!((point.x, point.y), (860, 340));
    }

    #[test]
    fn an_ambiguous_or_missing_region_yields_no_correction() {
        assert!(report().corrected_point("twins").is_none());
        assert!(report().corrected_point("gone").is_none());
    }

    #[test]
    fn a_report_without_optional_fields_still_parses() {
        let minimal = r#"{ "all_relocated": true,
          "results": [{ "label": "a", "found": true }] }"#;
        let parsed: FindReport = serde_json::from_str(minimal).expect("parses");
        assert!(parsed.is_confirmed("a"));
    }
}
