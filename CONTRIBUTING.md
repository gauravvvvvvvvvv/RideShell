# Contributing

Thanks for helping. RideShell is meant to be a dependable tool, so changes are judged first
on correctness, isolation and compatibility, and on cleverness last.

## Build

Requirements: Rust 1.80+ and a C compiler (SQLite is compiled in via `rusqlite`'s `bundled`
feature): MSVC Build Tools on Windows, Xcode command-line tools on macOS, `build-essential`
(or equivalent) on Linux.

```sh
cargo build                     # debug binary: target/debug/rideshell(.exe)
cargo build --release           # optimised binary: target/release/rideshell(.exe)
cargo install --path crates/rideshell-cli   # put `rideshell` on PATH (~/.cargo/bin)
```

On Windows, the single release executable (CLI + RideShell App bundled) is built with
`powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1`. The app alone:
`cargo run -p rideshell-app`. Its UI lives in `crates/rideshell-app/assets/` (plain HTML,
CSS and JavaScript, compiled in; there is no npm build step).

### Publishing a release (maintainer)

Double-click `scripts\release.cmd` (or run `scripts\release.cmd 0.2.0` for a specific version).
It bumps the version, runs the tests, builds `rideshell.exe` and the Linux tarball (in Docker),
signs them, and then asks before it commits, pushes and creates the GitHub release.
`scripts\release.cmd -DryRun` does everything except publishing and undoes the version bump.

Release notes come from a `## Unreleased` section in `CHANGELOG.md`. Without one, the commit
messages since the last release are used. Every release must be **signed**, or RideShell App
refuses it as an update; the script does this with the key below. CI never publishes releases,
because the signing key must not leave the maintainer's machine.

The signing key is made once with `rideshell-release keygen` and lives outside the repository
(`~/.rideshell-release/signing.key`, or the file in `RIDESHELL_SIGNING_KEY`). Its public half is
`RELEASE_PUBLIC_KEY` in `crates/rideshell-core/src/release.rs`. Never commit the secret key, and
keep a backup: without it, existing installs can't accept updates.

Try it without touching your real data:

```sh
export RIDESHELL_HOME=/tmp/rideshell-dev       # PowerShell: $env:RIDESHELL_HOME = "$env:TEMP\rideshell-dev"
cargo run -- init --yes --no-shell
cargo run -- profile create work
eval "$(cargo run -q -- hook bash)"            # load integration into this shell only
```

## Test

```sh
cargo test                      # unit + end-to-end tests
cargo clippy --all-targets      # lints (keep it warning-free)
cargo fmt                       # formatting
```

The end-to-end suite (`crates/rideshell-cli/tests/cli.rs`) runs the real binary with an
isolated `RIDESHELL_HOME`, and drives real shells when they are present:

* PowerShell: `powershell` and/or `pwsh` on PATH
* bash: on Windows, Git for Windows' bash (never `System32\bash.exe`, which is WSL).
  Elsewhere, `bash` on PATH. Override with `RIDESHELL_TEST_BASH=/path/to/bash`.
* zsh: `zsh` on PATH

Missing shells are skipped with a `SKIP` line (see it with `cargo test -- --nocapture`).
Please mention in your PR which shells and operating systems you ran the suite on. That is
how we keep the support table in the README honest. CI (`.github/workflows/ci.yml`) runs
formatting, Clippy, the tests and release builds on Windows and Linux for every push and pull
request; it has PowerShell 5.1, cmd and bash, so PowerShell 7, zsh and macOS still rely on
these local runs.

## Code layout

See [ARCHITECTURE.md](ARCHITECTURE.md). In short, logic goes in `rideshell-core`, and
argument parsing and printing go in `rideshell-cli`. The prompt hook (`hook-env`) is a hot
path: no network, no process spawning, no filesystem scans, never create state, never
write to stdout except emitted shell code.

## Adding an identity adapter

1. Read the tool's **current** documentation and find a supported way to select an
   account per process: an environment variable, config-directory selector or named
   profile. Link it in the module docs. If none exists, open an issue first.
2. Create `crates/rideshell-core/src/providers/<tool>.rs` implementing `Provider`.
   * `settings()` holds references only (names, paths), never secrets.
   * `isolation()` states the guarantee honestly.
   * `environment()` must be pure and fast (it runs in the prompt hook).
   * `detect()` may spawn the tool, but is never used by the hook.
3. Register it in `providers::registry()` and remove it from `providers::PLANNED`.
4. Add unit tests for `environment()`/`validate()`. Where possible add an end-to-end test
   with a local fixture (e.g. a fake config directory). Never fabricate a successful
   real login.

## Adding a shell adapter

1. Add a variant to `shells::Shell`, a template in `shells/scripts/`, and an `rc_file`
   location.
2. The script must check the `# rideshell:emit` marker before evaluating, preserve the
   previous exit status, be idempotent when sourced twice, and never fail the prompt.
3. Add an end-to-end test in `tests/cli.rs` mirroring the PowerShell and bash ones.

## Renaming the project

All user-visible names derive from `crates/rideshell-core/src/brand.rs` (product, binary,
env-var prefix, markers). Shell scripts are templated from it. A rename means editing that
file, the `[[bin]] name` in `crates/rideshell-cli/Cargo.toml`, the crate names, and the docs.

## Commit and PR guidelines

* Keep the build and tests green after every commit.
* Distinguish implemented / experimental / planned in docs and help text.
* Do not add dependencies without a reason in the PR description. Prefer small,
  well-maintained crates.
* Never hard-code credentials or personal paths, even in tests.
