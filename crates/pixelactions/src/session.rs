//! Reading a pixelcoords session — deliberately tolerant.
//!
//! The sister tool's schema grows by adding optional fields (provenance,
//! names, and whatever its roadmap lands next). Every such addition must
//! be a no-op here, so this reads through `pixelcoords-core`'s own types
//! — which ignore unknown fields — and gates only on the schema version
//! it understands. Strict parsing belongs to *our* config, not to
//! someone else's data. See AGENTS.md, "the compatibility contract".

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use pixelcoords_core::session::SessionFile;

/// The session schema this version of pixelactions understands.
pub const SUPPORTED_SCHEMA: u32 = 1;

/// Resolve a session path — a directory or a `session.json` — to the file.
pub fn session_json_path(path: &Path) -> PathBuf {
    if path.is_dir() {
        return path.join("session.json");
    }
    path.to_path_buf()
}

/// Load and validate a session.
pub fn load(path: &Path) -> Result<SessionFile> {
    let file = session_json_path(path);
    let text = std::fs::read_to_string(&file)
        .with_context(|| format!("cannot read {}", file.display()))?;
    let session: SessionFile = serde_json::from_str(&text)
        .with_context(|| format!("{} is not a pixelcoords session", file.display()))?;

    if session.schema > SUPPORTED_SCHEMA {
        bail!(
            "{} uses session schema {}, but this pixelactions understands {} — upgrade pixelactions",
            file.display(),
            session.schema,
            SUPPORTED_SCHEMA
        );
    }
    if session.selections.is_empty() {
        bail!(
            "{} describes no selections — nothing to act on",
            file.display()
        );
    }
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write fixture");
        path
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pixelactions-test-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    const MINIMAL: &str = r#"{
      "schema": 1,
      "app": { "name": "pixelcoords", "version": "0.1.1" },
      "created_utc": "2026-07-28T00:00:00Z",
      "monitors": [{ "index": 0, "name": "m", "primary": true,
        "origin_px": {"x":0,"y":0}, "size_px": {"w":100,"h":100}, "scale": 1.0 }],
      "selections": [{ "shape": "rect", "label": "submit", "monitor": 0,
        "px": {"x":10,"y":10,"w":20,"h":20},
        "global_px": {"x":10,"y":10,"w":20,"h":20},
        "crop": "crop-0-submit.png" }]
    }"#;

    #[test]
    fn a_directory_and_a_file_path_both_work() {
        let dir = temp_dir("paths");
        write(&dir, "session.json", MINIMAL);
        assert!(load(&dir).is_ok(), "directory path");
        assert!(load(&dir.join("session.json")).is_ok(), "file path");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_future_fields_are_ignored_not_fatal() {
        // The whole compatibility contract in one test: a session written
        // by a newer pixelcoords, carrying fields this build has never
        // heard of, must still load.
        let dir = temp_dir("forward");
        let future = MINIMAL.replace(
            r#""created_utc": "2026-07-28T00:00:00Z","#,
            r#""created_utc": "2026-07-28T00:00:00Z", "vibes": "immaculate", "measures": [],"#,
        );
        write(&dir, "session.json", &future);
        assert!(load(&dir).is_ok(), "unknown fields must not break us");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_newer_schema_version_is_refused_with_an_actionable_message() {
        let dir = temp_dir("schema");
        write(
            &dir,
            "session.json",
            &MINIMAL.replace(r#""schema": 1"#, r#""schema": 2"#),
        );
        let error = load(&dir).expect_err("should refuse").to_string();
        assert!(error.contains("upgrade pixelactions"), "message: {error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_session_with_no_selections_is_refused() {
        let dir = temp_dir("empty");
        let empty = MINIMAL.replace(
            r#""selections": [{ "shape": "rect", "label": "submit", "monitor": 0,
        "px": {"x":10,"y":10,"w":20,"h":20},
        "global_px": {"x":10,"y":10,"w":20,"h":20},
        "crop": "crop-0-submit.png" }]"#,
            r#""selections": []"#,
        );
        write(&dir, "session.json", &empty);
        assert!(load(&dir).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_session_names_the_path_it_looked_for() {
        let error = load(Path::new("/nonexistent/pixelactions")).expect_err("missing");
        assert!(format!("{error:#}").contains("/nonexistent/pixelactions"));
    }
}
