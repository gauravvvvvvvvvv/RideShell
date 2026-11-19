# Architecture

This document describes how RideShell is built, the boundaries between its parts, and the
reasons behind the main decisions. It covers what exists today (Milestone 1) and how
planned modules fit in. Planned items are marked *(planned)*.

## 1. Mental model

```
┌──────────────────────── terminal host (Windows Terminal, iTerm2, VS Code…) ───────────────────────┐
│  ┌──────────────── shell process (PowerShell / bash / zsh) ────────────────┐                      │
│  │  your commands: git, npm, aws, wrangler…   ← inherit the environment    │                      │
│  │                                                                          │                      │
│  │  rideshell shell adapter (tiny script)                                   │                      │
│  │    • prompt hook → `rideshell hook-env`  → prints set/unset statements   │                      │
│  │    • `rideshell` function → lets `profile use` change *this* shell       │                      │
│  └─────────────────────────────────┬────────────────────────────────────────┘                      │
└────────────────────────────────────┼──────────────────────────────────────────────────────────────┘
                                     │ short-lived process, ~15 ms
                         ┌───────────▼────────────┐
                         │  rideshell (one binary) │── SQLite (profiles, bindings, identities)
                         │  core + providers       │── config.toml (preferences)
                         └─────────────────────────┘
```

There is **no daemon** and no container: tools run as normal processes. The binary runs, answers, and exits. All per-terminal state lives in
that terminal's process environment, which the operating system already isolates. That one
decision is what makes concurrent sessions safe.

Three categories are kept distinct throughout: **terminal hosts** draw text, **shells**
interpret commands, and **tools** are what users run. RideShell integrates with shells and
tools only. It cannot (and does not try to) control a host's fonts, themes or windows.

## 2. Workspace layout

```
crates/
  rideshell-core/            library: everything except argument parsing and printing
    src/brand.rs             product/binary/env-var names (rename in one place)
    src/paths.rs             config/data directory discovery (RIDESHELL_HOME override)
    src/config.rs            config.toml (preferences only)
    src/store.rs             SQLite storage + migrations (src/migrations/*.sql)
    src/names.rs             validation of profile names / env keys
    src/secrets.rs           heuristics that refuse credential-looking values
    src/pathkey.rs           canonical, case-aware directory keys for bindings
    src/resolve.rs           profile precedence + explanation
    src/environment.rs       profile → full set of environment variables
    src/envstate.rs          reversible environment plans (RIDESHELL_STATE)
    src/exec.rs              isolated command execution
    src/profile_file.rs      portable TOML import/export
    src/providers/           identity adapters: claude, codex, gh (account dirs), git
    src/vault.rs             secrets in the OS credential store (keyring)
    src/shims.rs             tool shims: create/list/refresh, find the real tool
    src/shells/              shell adapters: quoting, emission, scripts, rc-file install
  rideshell-cli/             the `rideshell` binary: clap definitions, commands, output
    tests/cli.rs             end-to-end tests, including real PowerShell and bash sessions
```

Planned modules (history, navigation, directory map, dependency store, TUI) get their own
modules or crates, and depend on `rideshell-core`, never the reverse. The core must stay
small and fast because it runs on every directory change.

## 3. Data

### 3.1 Storage split

| Data | Where | Why |
|---|---|---|
| Preferences (`[prompt]`, `[shell]`) | `config.toml` in the config dir | Human-edited, rarely written, commented defaults |
| Profiles, env, identities, bindings, default profile | SQLite in the *local* data dir | Written from many terminals at once; needs transactions |
| Per-session selection and applied state | Process environment | Isolation for free; dies with the terminal |

The database is never placed in a roaming or synced directory (SQLite and file sync do not
mix). `RIDESHELL_HOME` puts both files in one directory, for tests and portable use.

### 3.2 Schema (v1)

`profiles(id, name UNIQUE NOCASE, description, created_at, updated_at)`
`profile_env(profile_id → profiles, key NOCASE, value)`
`identities(profile_id → profiles, provider, settings JSON, updated_at)`
`bindings(id, path, path_key UNIQUE, profile_id → profiles, created_at)`
`settings(key, value)` (holds `default_profile_id`)

v2 adds `profile_env.secret` and `dir_env(id, path, path_key, key, value, secret)`. Secret rows hold
no value; the value lives in the OS credential store (section 8b).

Foreign keys cascade, so deleting a profile removes its env, identities and bindings in the
same transaction. The default is stored by id, so renaming a profile keeps it.

### 3.3 Concurrency and durability

* WAL journal mode, `synchronous=NORMAL`, a 10 s busy timeout.
* Every read-modify-write runs in `BEGIN IMMEDIATE`, so writers queue instead of hitting
  upgrade deadlocks.
* Migrations: `PRAGMA user_version` with ordered embedded SQL files. The version is re-read
  inside the write transaction, so two processes starting at once cannot double-migrate.
  A database newer than the binary is refused with a clear message.
* Multi-row operations (duplicate, delete, import) are single transactions. An interrupted
  command leaves either the old state or the new one, never a mixture.
* Tested: 8 threads × 10 concurrent profile creations; 16 concurrent `run` processes.

## 4. Profile resolution

```
1. --profile <name>            (command-level override)
2. RIDESHELL_PROFILE=<name>    (session override, set by `profile use`)
3. most specific binding       (longest bound ancestor of the working directory)
4. default profile
```

* **Directory keys** are canonical paths (symlinks and junctions resolved, `\\?\` removed
  on Windows, `\`→`/` on Windows), lower-cased where the platform is case-insensitive
  (Windows, macOS default). Containment is component-wise: `/a/bc` is not inside `/a/b`.
* **No silent fallback.** If a level names a profile that does not exist (for example a
  session still pointing at a deleted or renamed profile), resolution stops and activates
  nothing. Falling through to another identity would be the worst possible failure for this
  tool.
* `resolve()` records every level's outcome (`selected`, `overridden`, `-`,
  `missing profile`). `rideshell profile why` prints exactly that.
* `shell.auto_apply = false` disables level 3 inside shells.

## 5. Environment planning (`envstate`)

A profile's desired environment is its identity adapters' variables, overlaid by its
explicit `profile env` variables (explicit wins, with a warning).

Applying it is a **plan** (`set` / `unset` lists) computed against the current environment
plus a bookkeeping record in `RIDESHELL_STATE`:

```json
{"p":"work","a":{"GIT_AUTHOR_NAME":{"o":"Mine","v":"Pat Work"},"AWS_PROFILE":{"o":null,"v":"corp"}}}
```

For each variable RideShell set, it remembers the **original** value (`o`) and the value it
**set** (`v`). On the next transition it:

1. restores originals for variables the new profile does not set, *unless* the user changed
   the variable by hand since (current ≠ `v`), in which case the user's value stays;
2. applies the new profile's variables, keeping the earliest known original;
3. updates `RIDESHELL_STATE` and `RIDESHELL_ACTIVE_PROFILE`.

Properties covered by unit tests: switching A→B never leaks A's variables; deactivating
returns exactly the pre-RideShell environment; re-applying the same profile is a no-op;
repeated switching never accumulates state; manual changes are respected.

The same planner serves the shell hook (`hook-env`) and `rideshell run`, so a command run
with `--profile personal` from a `work` shell gets personal's variables *and* has work's
variables undone first.

## 6. Shell integration

### 6.1 Contract

Each adapter is a small script generated by `rideshell hook <shell>` from a template in
`core/src/shells/scripts/`. It:

1. **Prompt hook.** Before each prompt, if the directory changed, it runs `rideshell
   hook-env` and evaluates its output. The output always starts with `# rideshell:emit`, and
   the script checks that marker, so stray text (help, errors) is never evaluated. Hook
   errors go to stderr and never break the prompt.
2. **`rideshell` function.** It wraps the binary. `rideshell profile use X` runs
   `profile use X --emit <shell>` and evaluates the result, which sets `RIDESHELL_PROFILE`
   in this shell only. After any `rideshell` command it forces a re-sync.
3. **Prompt indicator** (optional). It prefixes `[profile] `. The prefix is stripped and
   re-applied, never accumulated, and it coexists with prompt frameworks.
4. Sets `RIDESHELL_SHELL` so `status` and `doctor` know integration is active.

| Shell | Hook mechanism | Notes |
|---|---|---|
| PowerShell 5.1 / 7 | wraps `function prompt`; preserves `$LASTEXITCODE`; runs original prompt first | Load after oh-my-posh/starship. Emitted code is pure ASCII, see below |
| bash | prepends to `PROMPT_COMMAND` (string or bash 5.1 array); preserves `$?` | Git Bash: login shells read `~/.bash_profile`, so `shell install` warns if it does not source `~/.bashrc` |
| zsh | `add-zsh-hook precmd` | |
| fish, cmd | *(planned)* | cmd has no prompt hook. It will need `doskey`/AutoRun or the explicit `run` path only |

### 6.2 Quoting and encoding

* POSIX values use single quotes (`'` → `'\''`), which is safe for any byte except NUL.
* PowerShell: Windows PowerShell decodes native-command output with the console code page,
  so any non-ASCII byte would be mangled. Values are therefore emitted as **pure ASCII
  expressions** (`('J' + [char]0x00FC + 'rgen')`), which also neutralises PowerShell's
  typographic-quote handling. The executable path in startup files is encoded the same way.

### 6.3 Startup-file edits (`shell install`)

* The exact block is shown first. Nothing is written without confirmation (`--yes` is the
  explicit non-interactive approval; `--dry-run` only shows the change).
* The PowerShell profile path is obtained from PowerShell itself
  (`$PROFILE.CurrentUserAllHosts`), which respects OneDrive folder redirection and covers
  the VS Code host.
* The block is guarded (`if (Get-Command rideshell ...)` / `if command -v rideshell ...`),
  so deleting the binary leaves a working shell.
* Edits keep the BOM and line endings, write a timestamped backup, and replace the file
  atomically (temp file + rename). Uninstall removes exactly the marked block.

## 7. Isolated execution (`rideshell run`)

Resolve the profile, plan the environment from the current process, and launch:

* **Unix:** `exec(2)` replaces RideShell, so signals, job control, TTY and exit status
  are exactly those of the command.
* **Windows:** spawn and wait. RideShell installs a Ctrl+C handler that swallows the signal
  for itself only (handlers are not inherited, unlike `SetConsoleCtrlHandler(NULL, TRUE)`).
  The child shares the console and still receives Ctrl+C. The child's exit code is
  propagated.
* Commands are found via `PATH`/`PATHEXT`, so `npm` resolves to `npm.cmd`.

## 8. Identity adapters (providers)

```rust
trait Provider {
    fn id(&self) -> &'static str;                 // "claude"
    fn tool(&self) -> &'static str;               // executable it configures (for shims)
    fn isolation(&self) -> &'static str;          // stated guarantee, shown to users
    fn settings(&self) -> &'static [SettingSpec]; // non-secret settings
    fn secrets(&self) -> &'static [SecretSpec];   // e.g. api_key -> ANTHROPIC_API_KEY
    fn environment(&self, &Settings, &ProviderContext) -> Result<BTreeMap<..>>; // pure, fast
    fn required_dirs(&self, ..) -> Vec<PathBuf>;  // created before launch
    fn login_args / logout_args / status_args;    // the tool's own auth commands
}
```

Rules every adapter follows:

* Use **only mechanisms the tool supports**: environment variables, config-directory
  selectors, named profiles. Never read, copy or extract tokens.
* Settings are references. Secrets go through the vault.
* If clean per-process isolation is impossible, say so in `isolation()`.

### Account-directory providers (Claude Code, Codex, GitHub CLI)

These tools keep login, settings and history in one directory that an environment variable
relocates. Each RideShell account gets `<data>/accounts/<tool>/<account>` (account defaults
to the profile name), or an existing directory (`config_dir=~/.claude` to adopt a current
login).

| Tool | Variable | Evidence |
|---|---|---|
| Claude Code | `CLAUDE_CONFIG_DIR` | Verified on Windows (Claude Code 2.1): `claude auth status` reports the chosen directory and a separate `loggedIn` state; two processes with different values ran in parallel with different accounts. Widely used, but its status in Anthropic's official docs was unclear when checked. |
| Codex CLI | `CODEX_HOME` | Verified on Windows: a fresh `CODEX_HOME` reports "Not logged in" while the default is logged in. Used by OpenAI's CI guide. |
| GitHub CLI | `GH_CONFIG_DIR` | Documented in `gh help environment`. Not tested here (gh not installed). |

**Known gap:** on macOS, Claude Code stores OAuth credentials in the Keychain. Whether those
are separated per config directory has not been verified, so multi-account Claude Code on
macOS is unverified until then. API-key accounts (`ANTHROPIC_API_KEY` secret) are unaffected.

### Git

`GIT_AUTHOR_*` / `GIT_COMMITTER_*` and `GIT_SSH_COMMAND`. No git config file is modified.

### Any other tool

Profile/project variables, secrets and `shim add <tool>` cover any tool that reads its
account from an environment variable or API key.

## 8a. Environment layers

1. identity adapters of the active profile (e.g. `CLAUDE_CONFIG_DIR`)
2. the profile's variables and secrets
3. directory (project) variables and secrets, shallow → deep

Later layers win, so a project's own API key overrides the account's key inside that
project. Secrets stay *references* (`EnvValue::Secret { vault_id }`) until launch.

## 8b. Secrets (vault)

* Backend: `keyring` crate. Windows Credential Manager, macOS Keychain, Linux Secret
  Service. Service `rideshell`, entry ids `profile/<profile_id>/<KEY>` and `dir/<row_id>`
  (ids survive renames).
* The database stores only `secret = 1` with an empty value.
* **Materialised only on launch** (shims, `run`, `auth`). The prompt hook calls
  `materialize(env, None)` and never sees secret values. `RIDESHELL_STATE` stores FNV
  fingerprints of applied values, not the values.
* Profile delete, duplicate and `uninstall --purge` keep the vault in sync.
* `RIDESHELL_INSECURE_TEST_VAULT=<file>` selects a plain-file backend for automated tests.

## 8c. Tool shims

* `<data>/shims/<tool>.exe` is a **hard link of `rideshell.exe`** (copy if hard links are
  impossible). Unix uses a symlink. No `.cmd` wrappers, so argument quoting and Ctrl+C
  behave like the real tool.
* `main()` checks the name it was invoked as. If that is not `rideshell`, it runs as the shim
  for that tool. The real tool is found via `which_all`, skipping the shims directory and
  RideShell itself.
* The shim resolves the profile (session → binding → default), materialises the environment
  with secrets, creates account directories, prints one dim notice line when stderr is a
  terminal (`shims.notify`), and `exec`s / spawns the real tool.
* It **fails closed**: a session naming a missing profile is an error, never a fallback to
  another account. `RIDESHELL_SHIM_BYPASS=1` runs the real tool untouched.
* Overhead is about 20 ms per launch (release build, Windows).
* Shims are on PATH via the installer (user PATH, all terminals including cmd) and via shell
  integration (front of PATH, which beats system-PATH tools in PowerShell and bash).
* Windows cannot overwrite a running executable. Shims and the installed exe are renamed
  aside and replaced, and leftovers are cleaned up next time.

## 8d. Installer (Windows)

`rideshell.exe` installs itself: by double-click (detected via `GetConsoleProcessList() == 1`)
or `rideshell install`. Per user, no admin rights:

* copies itself to `%LOCALAPPDATA%\Programs\RideShell\rideshell.exe`;
* prepends `shims` and the program folder to `HKCU\Environment\Path`, preserving
  `REG_EXPAND_SZ` and existing entries, and broadcasts `WM_SETTINGCHANGE`;
* registers `HKCU\...\Uninstall\RideShell` (Apps & Features → `uninstall --program`);
* refreshes shims to point at the installed copy.

`uninstall --program` reverses each step. The program folder is deleted by a detached
`cmd` after the process exits. Tests redirect the registry with
`RIDESHELL_TEST_REGISTRY_ROOT` and never touch the real PATH.

**Limitation:** Windows evaluates the system PATH before the user PATH. In plain cmd.exe,
a tool installed in a *system* PATH directory wins over its shim. Claude Code's native
installer and npm global installs use user directories, so they are covered. Shell
integration fixes this for PowerShell and bash.

## 8e. RideShell windows and command routing

`rideshell open` (the default with no arguments) starts the session host, PowerShell 7 or
Windows PowerShell, with `-NoExit -EncodedCommand` loading `rideshell hook <shell> --session`.
The host is unmodified PowerShell with the user's profile, so fidelity is PowerShell's own.
`--session` appends `shells/scripts/rideshell-session.ps1`:

1. **Enter key** (PSReadLine handler). Incomplete input (open braces) is accepted as usual.
   Otherwise PowerShell parses the line (`Parser.ParseInput`) and resolves every command name
   (`Get-Command`). Lines made only of cmdlets/functions that parse cleanly take a fast path.
   Anything else goes to `rideshell route` with the line and these hints in environment
   variables (UTF-16 safe, no argument quoting).
2. **Classification** (`rideshell_core::dialect`, unit-tested): explicit prefix, then bash
   rules (only if Git Bash exists), then cmd rules, else PowerShell. A rule only moves a line
   when it is invalid or means something different in PowerShell. Examples: `/s` switches on
   aliased built-ins, `set X=1`, `%VAR%`, `>nul`, `&&` that does not parse, POSIX flags that
   are not a prefix of the aliased cmdlet's parameters, POSIX tools PowerShell cannot find.
3. **Execution** (`rideshell dialect-exec cmd|bash --console`). cmd gets
   `/d /s /c "<line>& call rideshell __capture <file> %^errorlevel%"`. bash runs a temporary
   script that captures before and after. `__capture` records cwd, environment and exit code
   as JSON. The child is attached to `CONIN$`/`CONOUT$` directly, because PowerShell captures
   the output of native commands started from key handlers.
4. **Sync back.** The difference (cwd, variables set or removed, compared case-insensitively;
   `=C:`-style and bash bookkeeping variables skipped) is written as ASCII-only PowerShell and
   applied, then the profile hook re-syncs. The line goes into PSReadLine history, and the
   prompt is redrawn below the output (`InvokePrompt` at the cursor row).

Limits: each routed line runs in a fresh cmd/bash process, so cmd `setlocal` or bash
functions do not persist between lines (cwd and environment variables do). A line that exits
the shell (`exit /b 3`) reports its code but brings no state back. cmd lines with unbalanced
quotes can break the capture suffix.

**Administrator** (`rideshell admin`): `ShellExecuteExW` with the `runas` verb on
`rideshell.exe open --dir <cwd> [--profile <session profile>]`, or `run --pause --dir ...
-- <cmd>`. Elevated processes do not inherit the caller's environment, so everything is
passed as arguments, and a session pointing at a missing profile is refused before elevating.
With the usual UAC setup (same account, elevated token), the elevated window uses the same
`%APPDATA%`, database and Credential Manager. If a *different* administrator account
approves the prompt, it has its own (empty) RideShell data, and a passed `--profile` fails
closed.

**Launchers:** the installer writes `RideShell.lnk` and `RideShell (Administrator).lnk`
(same target, `SLDF_RUNAS_USER` set in the header) to the user's Start menu, plus a Windows
Terminal JSON fragment. Uninstall removes them. A double-click on the *installed* copy opens a
window; any other copy runs the installer.

## 8f. RideShell App (`crates/rideshell-app`)

* **Window:** tao, undecorated, with a custom title bar and resize edges handled through IPC,
  plus Windows 11 rounded corners (DWM). **UI:** wry on WebView2. All assets (HTML, CSS, JS,
  xterm.js, Geist fonts, icon) are `include_bytes!`'d and served from the custom protocol
  `rs://` (`http://rs.localhost` on Windows). Nothing is loaded from the network.
* **Tabs:** each tab is a ConPTY (portable-pty) running `rideshell open --no-banner
  [--profile p] [--command line]`, so a tab is exactly a RideShell window session. Output
  reaches the page as base64 chunks through `evaluate_script`. Input, resize and kill go back
  as JSON IPC messages.
* **Live tab state:** with `TERM_PROGRAM=RideShell`, the session's prompt also emits the
  invisible `OSC 7777;profile=<p>;admin=<0|1>;cwd=<path>`, which labels tabs and drives the
  sidebar and status bar.
* **Account switching:** sends `Esc` (clears the line in PSReadLine) and
  `rideshell profile use <name>` to the tab, the same path a typed command takes.
* **Installed tools** (`rideshell_core::tools`): a directory scan of PATH. The first match
  per name wins, in PATHEXT order. Each directory is classified by path (npm, winget, scoop,
  cargo, python, git, store, program...). Nothing is executed. *Route* = `shims::create`.
* **Program icons:** `/exe-icon?p=<path>` on the custom protocol returns the executable's
  own icon as PNG (`PrivateExtractIconsW` at 64 px, `GetDIBits`, alpha taken from the mask for
  old icons, cached). No icon (scripts, Store aliases) gives a 404, and the page keeps its
  neutral placeholder. Git and Git Bash use `git-bash.exe`'s icon from the same install.
* **Files panel:** `listDir` (hidden/system entries skipped, folders first, capped at 1000),
  places (`dirs` known folders + directory bindings), pinned and recent folders in `app.json`.
  Going to a folder types `Set-Location -LiteralPath '<path>'` into the tab when it is at a
  prompt (known from the last OSC 7777), else opens a new tab. Folder picker: `rfd` on a
  worker thread. Drag and drop: wry's drop handler.
* **Explorer menu:** `HKCU\Software\Classes\{Directory,Directory\Background,Drive}\shell\RideShell`
  → `"rideshell-app.exe" --dir "%V"`. Added by `install`, removed by `uninstall --program`.
* **Settings:** appearance, sidebar and themes are stored in `<config>/app.json` (Windows
  Terminal colour-scheme format, so WT schemes import as-is). Shell settings are written to
  `config.toml` through `Config::save`.
* **Packaging:** `scripts/build-release.ps1` builds the app, then builds `rideshell.exe` with
  `RIDESHELL_APP_EXE` set, so `build.rs` embeds the app. `install` writes it next to
  `rideshell.exe`. Start menu: `rideshell-app.exe` (normal) and `rideshell-app.exe --admin`
  with `SLDF_RUNAS_USER` (Administrator). The arguments must differ, because the Start menu
  merges shortcuts with identical command lines.
* **Admin:** the app re-launches itself with `runas`. An elevated window is separate, since a
  non-elevated process cannot host an elevated ConPTY child.

## 8g. Usage stats, optional counter and message slot (`assets/usage.js`)

* **Local stats:** a minute counts as active when the window has focus or there was input in
  the last 5 minutes. Counts go to `app.json` under `usage.days` (180 days kept) along with
  `sessions`. They are shown in Settings → Privacy & usage.
* **Counter:** compiled in only with `RIDESHELL_TELEMETRY_URL` (`option_env!`, `https://`
  only). It is disabled by `DO_NOT_TRACK` / `RIDESHELL_NO_TELEMETRY` (checked by the host) or
  by the Settings toggle. `POST /v1/ping {id, v, os, m}` is sent every 5 minutes with
  `credentials: 'omit'` and `referrerPolicy: 'no-referrer'`. `id` comes from
  `crypto.getRandomValues`, 128 bits, regenerated on every new UTC day. A notice appears on
  first start. The server (`server/src/index.js`) keeps one row per `(day, id)`. "Live" means
  `last_seen` within the last 10 minutes.
* **Message slot:** compiled in only with `RIDESHELL_PROMO_URL`. `GET` returns
  `{text, url, until}` every 6 hours, fetched without an ID. The text is set with
  `textContent` and capped at 120 characters. The link must be `https://` and opens through
  the host.

## 9. Security boundaries

See [SECURITY.md](SECURITY.md). In short: RideShell stores no credentials, refuses values
that look like secrets, never edits files without approval, keeps per-session state
in-process, and fails closed when a selected profile is missing.

## 10. Performance

* The prompt hook runs only when the working directory changes or after a `rideshell`
  command. Measured on Windows 11 (release build): `hook-env` ≈ 17 ms, against a ≈ 13 ms
  floor for just spawning any process. The cost is one SQLite open plus a few small queries.
* No filesystem scanning in the hot path. Binding lookup is a canonicalise plus an
  in-memory longest-prefix match over the bindings table.
* Release binary is about 4.4 MB with SQLite statically linked. No runtime dependencies.
* Shim launch overhead is about 20 ms (`claude --version`: 33 ms direct, 52 ms via shim).

## 11. Planned modules and where they attach

| Module | Attaches via | Notes |
|---|---|---|
| Command history | shell adapters gain a pre/post-exec hook → `rideshell history record` | Separate tables. Opt-out per profile and directory, redaction, retention. Never replaces native history |
| Directory/project activity | the same hook (cwd changes) | Kept separate from shell history and from git history |
| Git activity | reads repositories on demand (`git` CLI or `gix`) | Never in the prompt hot path |
| Directory map / TUI | new crate on `ratatui` | Lazy traversal |
| Dependency store | separate crate, separate lifecycle | Analysis first. Optimisation via pnpm's content-addressable store. Never modifies projects without preview + approval |
| Workspace files | `.rideshell.toml` in a repo (portable, no secrets) + machine-local DB bindings | |
| Extensions | provider/shell traits first, then an out-of-process plugin protocol | Avoid dynamic loading in the core |
