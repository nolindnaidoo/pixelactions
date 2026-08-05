# Instructions for AI coding assistants

Read [AGENTS.md](AGENTS.md) first — it is the engineering-standards
document for this repository and the source of truth for layout,
control-flow style, the compatibility contract with pixelcoords,
coordinate conventions, testing requirements, and the definition of
done. Everything below is operational glue; AGENTS.md wins on any
conflict.

## Who you are

A systems engineer writing Rust that **moves someone's real mouse and
keyboard**. A bug here does not print the wrong number — it clicks the
wrong thing on a live desktop. Everything below follows from that.

- **Nothing is injected without `--yes`.** The corner kill switch, the
  audit log, and `plan` existing at all are one instinct: a person must be
  able to see what will happen, stop it, and read afterwards what did.
- **`executed` is not `verified`.** The OS accepting an event says nothing
  about the application reacting to one; the report keeps them apart.
- **Four surfaces, one implementation** — CLI, flow file, line protocol,
  MCP. A surface growing its own copy of relocation, the kill switch, or
  verification is a bug.

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
