# Roadmap

Status legend: ✅ done · 🧪 implemented, not yet tested on that platform · 🔜 next · 📋 planned

## Milestone 1: Working terminal foundation ✅

* ✅ Rust workspace (`rideshell-core` library + `rideshell` binary), MIT license
* ✅ Local configuration (`config.toml`) with commented defaults
* ✅ SQLite database with WAL, busy timeout, transactional migrations
* ✅ Profiles: create, list, show, edit, rename, duplicate, delete, default, import/export (TOML)
* ✅ Profile environment variables with secret-value refusal
* ✅ Directory bindings with nested/most-specific resolution, symlink/junction canonicalisation,
  case-insensitive matching on Windows/macOS
* ✅ Precedence model (flag > session > binding > default) with `profile why`
* ✅ Per-session selection (`profile use`) via process environment, so terminals are isolated
* ✅ Reversible environment application (originals restored; manual edits respected)
* ✅ `rideshell run --profile P -- cmd` (exec on Unix, spawn + exit-code passthrough on Windows)
* ✅ Git identity adapter (author/committer + SSH key via `GIT_SSH_COMMAND`)
* ✅ Shell integration: Windows PowerShell 5.1 ✅, Git Bash ✅, PowerShell 7 🧪, zsh 🧪,
  Linux/macOS bash 🧪
* ✅ Reversible startup-file install/uninstall with preview, backup, atomic write
* ✅ `init`, `status`, `doctor`, `config`, `uninstall [--purge]`
* ✅ Prompt indicator `[profile]` (configurable, `RIDESHELL_PROMPT=0` per session)
* ✅ Unit + end-to-end tests, including real PowerShell and bash sessions and concurrency

## Milestone 2: Multi-account tools ✅ (Windows) / 🧪 (macOS, Linux)

* ✅ Claude Code accounts (`CLAUDE_CONFIG_DIR`). Verified on Windows: separate login state, parallel sessions
* ✅ Codex CLI accounts (`CODEX_HOME`). Verified on Windows: separate login state
* ✅ GitHub CLI accounts (`GH_CONFIG_DIR`, documented). 🧪 gh not installed on the test machine
* ✅ `auth login/logout/status` running each tool's own sign-in per account
* ✅ Secrets in the OS credential store (Windows Credential Manager tested; Keychain and Secret
  Service 🧪), per profile and per project directory, injected only into launched tools
* ✅ Project (directory) variables, layered over profile variables
* ✅ Tool shims (`claude`, `codex`, `gh`, any tool via `shim add`), working in PowerShell,
  Git Bash and cmd.exe. They fail closed for missing profiles
* ✅ Windows self-installer (double-click), user PATH, Apps & Features, clean uninstall
* 📋 Verify Claude Code login separation on macOS (Keychain) and Linux
* 📋 SSH adapter (beyond Git): per-profile `IdentityFile` / config include
* 📋 Cloudflare Wrangler, AWS and Vercel adapters (API-token variables already work via `secret set`)
* 📋 Sensitive-command guard (confirm before e.g. `git push` / `wrangler deploy` when the
  active profile differs from the directory's binding)
* 📋 Test PowerShell 7, zsh, Linux and macOS (manually or on local VMs; no hosted CI planned)

## Milestone 2b: RideShell window ✅ (Windows)

* ✅ `rideshell` opens a RideShell window: real PowerShell host plus profiles, shims and routing
* ✅ Automatic cmd / bash routing per line, with cwd, environment and exit code synced back.
  Tested in a real interactive console (Windows PowerShell 5.1 host)
* ✅ `@cmd` / `@bash` / `@ps` prefixes, `rideshell dialect` explanation
* ✅ `rideshell admin`: Administrator window with the same profile and folder; `admin -- cmd`
* ✅ Start menu entries (normal + Administrator) and a Windows Terminal profile
* 🧪 PowerShell 7 as the host
* 📋 Persistent cmd/bash state beyond cwd and environment (e.g. `setlocal`, bash functions)
* 📋 macOS/Linux window (zsh/bash host with PowerShell-style routing is not planned; native shells already work)

## Milestone 2c: RideShell App ✅ (Windows)

* ✅ Windowed terminal (WebView2 + xterm.js + ConPTY), tabs, Geist fonts, custom title bar
* ✅ One-click account switching, per-account tool rows (sign in / status / connect)
* ✅ Launchers, customisable (hide built-ins, pin any installed tool)
* ✅ Installed-tools browser and `rideshell tools` (PATH scan with sources), per-tool routing
* ✅ Command palette, settings page (appearance, terminal, sidebar, shell, accounts, advanced)
* ✅ 11 colour schemes + Windows Terminal scheme import; light schemes with contrast adjustment
* ✅ Administrator window, single-exe bundle, app icon
* ✅ Real program icons (from each .exe) with a neutral placeholder fallback
* ✅ Files panel (places, projects, pinned/recent, browse), folder picker, drag and drop,
  Explorer "Open in RideShell"
* 📋 Split panes, search in scrollback, per-tab colour, drag to reorder tabs
* 📋 macOS / Linux build of the app (tao/wry support them; ConPTY parts are Windows-only today)

## Milestone 2d: Privacy-preserving metrics & message slot ✅

* ✅ Local usage dashboard (Settings → Privacy & usage), stored only on the user's PC
* ✅ Optional anonymous user count (daily-rotating ID, no IPs), build-time opt-in, user opt-out
* ✅ Reference server + owner dashboard (`server/`, Cloudflare Worker + D1)
* ✅ Optional one-line status-bar message slot (no ID, cookies or referrer)
* 📋 Signed MSIX package: top-level "Open in RideShell" in Windows 11's compact menu, no SmartScreen warning

## Milestone 3: Project intelligence 📋

* Project detection (git root, manifest files), workspace metadata
* Optional `.rideshell.toml` workspace file (portable, secret-free) + machine-local overrides
* Git repository discovery
* Directory history (recent/frequent), `rideshell jump`, `recent`, `projects`
* Project-aware command history: pre/post-exec hooks, exit status, duration, profile,
  redaction, per-profile/per-directory opt-out, retention

## Milestone 4: Interactive terminal utilities 📋

* `ratatui`-based profile selector, history explorer, workspace overview
* `rideshell map`: lazy directory explorer with git/profile/activity indicators
* Configurable prompt segments; dark theme

## Milestone 5: Shared dependency system 📋

* `deps status/analyze`: find duplicate `node_modules` packages across projects and estimate
  savings (read-only)
* Opt-in optimisation built on pnpm's content-addressable store; preview + approval before
  any change; never breaks npm/Yarn/pnpm workflows

## Milestone 6: Broader compatibility and extensions 📋

* fish integration
* cmd.exe integration (limited by cmd's lack of a prompt hook)
* Extension SDK (provider and shell adapter interfaces, out-of-process plugins)
* Packaging: signed prebuilt binaries, winget/Scoop/Homebrew (code signing avoids SmartScreen warnings)

## Not planned

* Replacing the user's shell or terminal (a standalone host may be explored much later)
* Cloud sync or accounts
* Collecting what users do (commands, folders, tools). The optional user count stays anonymous
  and daily-rotating (see SECURITY.md)
