# Working in this repo

`cardputer-adv` is board support for the M5Stack Cardputer ADV (ESP32-S3) on esp-hal. Two apps use it from crates.io: [backgammon](../backgammon) and [chord-detector](../chord-detector). To try a change in them before it's published, each takes a local, uncommitted `[patch.crates-io]` with the path to this repo (their CLAUDE.md or firmware `Cargo.toml` say where). The API is in [README.md](README.md); changes go in [CHANGELOG.md](CHANGELOG.md).

## Before committing

- `just` (fmt check, and clippy for the Xtensa target with no features and with all of them, and on the example) must pass, and `just build` and `just doc` (warnings are errors). CI (`.github/workflows/ci.yml`) runs the same recipes, plus `just package`, and publishes the docs to GitHub Pages from `main`.
- Then build both apps against the change, with that local `[patch.crates-io]` in each: `just build` and `just clippy` in backgammon, `just cardputer build` and `just cardputer clippy` in chord-detector. A breaking change updates their call sites in the same session.
- Hardware behaviour can only be checked on a Cardputer: `just hardware` flashes `examples/hardware.rs`, which tries each part and logs what it does, or flash one of the apps. A new part gets a line in that example.

## Conventions

- **Lints:** clippy pedantic and nursery. Warnings are errors, `unsafe_code` is forbidden, `missing_docs` warns.
- **Idiomatic Rust, always:** every change, however small, reads as idiomatic Rust. Clippy passing isn't enough:
  - `?`, `if let`/`let … else` and let-chains, not a `match` that only unwraps one case; iterator chains rather than a loop that sets a flag and breaks.
  - Implement the standard traits: `From` for conversions, `Display` for text, `core::error::Error` for error types.
  - Imports grouped in braces per crate, never fully-qualified paths in the code.
  - Silence a lint with `#[expect(lint, reason = "…")]`, not `#[allow]`.
  - `///` docs on every public item, with `# Errors` and `# Panics` where they apply.
- **No app code:** nothing specific to either app belongs here. It's meant for crates.io (MIT OR Apache-2.0).
- **`no_std`, no allocator.** Statics for what's set up once (and a `# Panics` for calling it twice).

## Don't, without asking

- Publish (`cargo publish`), push, or create remotes.

## Git

Commit as the user's personal identity (`tszarzynski@gmail.com`), set in `.git/config`.
