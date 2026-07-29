# Instructions for AI coding assistants

Read [AGENTS.md](AGENTS.md) first — it is the engineering-standards
document for this repository and the source of truth for layout,
control-flow style, the compatibility contract with pixelcoords,
coordinate conventions, testing requirements, and the definition of
done. Everything below is operational glue; AGENTS.md wins on any
conflict.

- Before declaring any change complete, run exactly what CI runs:
  `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`. All three must pass.
- Never add inline `#[allow(...)]` — fix the lint, or add a commented
  relaxation to `[workspace.lints]` in the root `Cargo.toml`.
- New logic goes in `pixelactions-core` when it is platform-free (it must
  then be unit-tested, 90% module coverage floor), and in the binary only
  when it needs the OS.
- **Never reimplement pixelcoords' geometry, capture, or matching.** Call
  the crate or the binary. If something is missing there, the fix belongs
  in that repo.
- Session parsing stays tolerant of unknown fields; our own flow parsing
  stays strict. Both are deliberate — see the compatibility contract.
- Input synthesis cannot be verified headless: build and test what you
  can, and state plainly what needs a manual run on real hardware —
  never claim an action works without having run it.
