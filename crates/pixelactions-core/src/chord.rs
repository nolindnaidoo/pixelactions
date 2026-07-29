//! Reading a key chord: `cmd+shift+s` → modifiers, then the key.
//!
//! This is pure string work, so it lives here rather than beside the
//! injector that consumes it. Keeping it in the binary meant it was
//! reachable only from a macOS-gated module, which made it dead code on
//! every other platform — the sort of thing a Mac-only workflow never
//! notices and a cross-platform CI leg catches immediately.

/// Why a chord could not be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ChordError {
    #[error("chord {0:?} names no key — expected something like \"cmd+s\" or \"enter\"")]
    Empty(String),
}

/// Split a chord into its modifiers and its final key: `cmd+shift+s` →
/// `(["cmd", "shift"], "s")`.
///
/// Whitespace around the parts is tolerated because chords are
/// hand-written in flow files, where `cmd + s` is a reasonable thing to
/// type. An empty chord is an error rather than a no-op: a step that
/// presses nothing is a typo, not an instruction.
pub fn split(chord: &str) -> Result<(Vec<&str>, &str), ChordError> {
    let mut parts: Vec<&str> = chord
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    let key = parts
        .pop()
        .ok_or_else(|| ChordError::Empty(chord.to_string()))?;
    Ok((parts, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_split_into_modifiers_and_a_key() {
        let (modifiers, key) = split("cmd+shift+s").expect("valid");
        assert_eq!(modifiers, vec!["cmd", "shift"]);
        assert_eq!(key, "s");
    }

    #[test]
    fn a_bare_key_has_no_modifiers() {
        let (modifiers, key) = split("enter").expect("valid");
        assert!(modifiers.is_empty());
        assert_eq!(key, "enter");
    }

    #[test]
    fn whitespace_around_chord_parts_is_tolerated() {
        let (modifiers, key) = split("cmd + s").expect("valid");
        assert_eq!(modifiers, vec!["cmd"]);
        assert_eq!(key, "s");
    }

    #[test]
    fn an_empty_chord_is_an_error() {
        assert!(split("").is_err());
        assert!(split("+").is_err());
        assert!(split("  ").is_err());
    }

    #[test]
    fn the_error_quotes_the_chord_it_could_not_read() {
        let error = split("+").expect_err("empty");
        assert!(error.to_string().contains("\"+\""), "{error}");
    }
}
