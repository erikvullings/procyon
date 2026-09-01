# Repository Guidelines

This repository implements a Rust SDK for EMF, EMF+, and WMF parsing/writing.
Keep this file short and stable. Treat the Microsoft specs as the source of
truth and sibling implementations as behavioral references.

## Project Stage

Version 0.1.0 is published and 0.2.0 development is underway. The goal remains
full typed coverage of EMF/EMF+/WMF fields with read/write roundtrip support.

Prefer this order:

1. model known records and fields as Rust types
2. preserve raw only for unknown records, unknown extensions, padding, blobs, or
   fields that need raw/text state for roundtrip
3. keep the raw record container as a fallback while typed coverage grows

Do not integrate this crate into `../ooxmlsdk` until the local API and
roundtrip behavior are more mature.

Parsing and writing are compatibility-first by default. Preserve input bytes
when typed reconstruction would lose information, expose that through
compatibility diagnostics, and keep strict validation explicitly opt-in.

## Primary References

Spec truth:

- `../ooxmlsdk-test-suite/references/[MS-EMF]-240423.docx`
- `../ooxmlsdk-test-suite/references/[MS-EMFPLUS]-240423.docx`
- `../ooxmlsdk-test-suite/references/[MS-WMF]-240423.docx`

Searchable Markdown is generated under
`../ooxmlsdk-test-suite/references/references/`. DOCX preserves more table and
heading structure; if extraction looks odd, verify the DOCX before changing
semantics.

Implementation references:

- `../core`: LibreOffice EMF/WMF/EMF+ behavior and compatibility handling
- `../poi`: Apache POI record models and parser behavior
- `../ooxmlsdk`: current temporary EMF/WMF usage to replace later
- `../ooxmlsdk-test-suite`: full corpus and Office-embedded roundtrip tests,
  plus the fixture/license boundary

Use local checkouts first. Browse only when local sources are missing or clearly
insufficient.

## Design Rules

- Keep fixed-layout structs as plain Rust structs.
- Use `#[derive(SdkObject)]` only for simple fixed field order read/write.
- Use `#[derive(SdkEnum)]` only for numeric one-of enums.
- Do not add a macro when ordinary Rust types/functions express the rule clearly.
- Do not build a schema/codegen layer unless there is a concrete maintenance
  need.
- Do not macro-generate record dispatch, offsets, bitmap payloads, strings, or
  EMF+ optional data. Hand-written code is clearer and easier to tune.
- Use `bitflags` directly for flags. Preserve unknown bits with
  `from_bits_retain`.
- Avoid `String` unless the API needs editable text. Use `SdkString::Raw/Text`
  for encoded strings that need roundtrip behavior.
- Avoid cloning/owning binary data unless it must be editable or outlive the
  source buffer.
- Unknown records should remain raw. Known records should be typed as much as
  possible, with raw fields only for necessary extensions/padding/blob data.

## Dev Loop

Run commands from the repository root.

### Command Discipline

- Run Cargo commands sequentially; do not start another Cargo command while one
  is still running.
- Do not set `CARGO_TARGET_DIR`; use the default workspace `target/`.
- After starting a Cargo command, wait for the final result before inspecting
  diffs, launching follow-up checks, or starting another repository command.
- If Cargo reports a target lock or another Cargo process is active, wait for
  that Cargo command to finish. Do not start competing commands to probe it.
- Do not create temporary Cargo projects, helper manifests, or throwaway crates
  for debugging. Use this workspace's tests and existing source instead.

Default loop:

1. `cargo fmt --all`
2. `cargo test`

Before broader changes or review:

1. `cargo fmt --all`
2. `cargo test --all-features`
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings`

This repository is currently small. Do not add heavier lanes unless the
workspace grows or a change needs them.

## Corpus And Tests

- Keep focused unit tests next to the code they protect.
- Full corpus roundtrip tests belong in `../ooxmlsdk-test-suite`, not this repo.
- Roundtrip tests must exercise typed parse/write and exact byte comparison;
  compatibility fallbacks and scan failures must be counted, never silently
  skipped.
- Temporary local corpus copies are acceptable during exploration, but remove
  them before finalizing changes.
- Do not add copyrighted corpus files to this repository.

## Cargo.lock

Commit `Cargo.lock` for this workspace. This is an application-style SDK
workspace with tests and derive tooling, and lockfile stability is useful during
pre-1.0 development.

## Git Guidance

- Inspect `git status --short` before summarizing work.
- Do not stage, commit, amend, or rewrite history unless explicitly asked.
- If asked whether changes can be committed, report verification status and
  suggest a commit message.
- Keep commit subjects short, imperative, and based on repository state.

Suggested subject style:

- `Improve compatible metafile round trips`

## Common Pitfalls

- EMF record sizes are bytes; WMF record sizes are WORDs.
- EMF+ records use `Type/Flags/Size/DataSize` and usually live inside
  `EMR_COMMENT_EMFPLUS`.
- Some spec fields are offsets from the record start, not from payload start.
- DIB bitmap offsets point to record-relative bitmap buffers.
- WMF and ANSI EMF strings are not guaranteed to be UTF-8.
- Reserved fields and padding may be nonzero in real files; preserve raw only
  where typed reconstruction would lose data.
