# Changelog

## 0.1.0 (2026-11-20)

First release. Windows 11 is the tested platform.

**Latest fixes and additions**
* Tool shims (`claude`, `codex`, ...) only apply a profile inside RideShell: a RideShell window,
  a shell with RideShell's integration loaded, or an explicit `RIDESHELL_PROFILE`. In a plain
  PowerShell, cmd or bash they run the real tool untouched.
* New Claude Code / Codex accounts share conversation history with the original folder by
  default (own sign-in kept; `rideshell history separate` undoes it).
* App: switching accounts while a program (such as Claude Code) runs in the tab opens the account
  in a new tab in the same folder, instead of typing the switch command into that program.
* App: state refreshes and PATH scans run off the UI thread, so terminals never wait for them.
* App: while Spotify is playing, the status bar shows the song with previous / play-pause / next
  (Windows media controls: no sign-in, nothing opened; it disappears when Spotify closes).
* Uninstalling no longer flashes a console window while it removes the program folder.

**Accounts**
* Profiles with folder bindings, precedence (flag > session > folder > default) and `profile why`
* Separate Claude Code, Codex and GitHub CLI accounts per profile (`CLAUDE_CONFIG_DIR`,
  `CODEX_HOME`, `GH_CONFIG_DIR`), running side by side. The first profile keeps existing logins,
  and new profiles start fresh.
* Git identities, plain variables per profile and per project
* Secrets in the OS credential store, per profile or per project, injected only into launched tools
* `auth login/logout/status` with a sign-in guide, `--email` and an optional `--browser private`
* Tool shims: `claude`, `codex`, `gh` and any `shim add <tool>` use the right account in every terminal

* Connect any installed tool to an account. RideShell reads the tool's own help to pre-fill
  its sign-in command and the variables it mentions (`rideshell inspect <tool>`), and you
  confirm them. A tool's config-folder variable reaches only that tool.
* Claude Code / Codex conversation history per account: adopt the original folder, or share
  history while keeping a separate sign-in (`rideshell history`)

**Shells**
* RideShell window: real PowerShell where cmd and bash lines run in their own shell, one
  line after another, with directory, variables and exit code carried over
* Shell integration for PowerShell 5.1/7, bash and zsh (prompt indicator, auto-switch on `cd`)
* `rideshell admin`: Administrator windows that keep the profile and folder

**RideShell App**
* Tabs (WebView2 + xterm.js + ConPTY), Geist fonts, custom title bar, command palette
* Accounts sidebar with one-click switching, sign-in guide and tool rows
* Launchers with real program icons and official installers for missing tools, plus
  pinning any installed tool
* Installed-tools browser (`rideshell tools`) with per-tool routing
* Files panel: favorites, places, projects, recently worked (RideShell, VS Code, Cursor,
  Claude Code, Codex), browse, folder picker, drag and drop
* Explorer "Open in RideShell" (folders, backgrounds, drives), with an optional classic menu
* Settings page: 11 colour schemes and Windows Terminal scheme import, terminal, sidebar,
  shell, accounts, privacy and usage, advanced
* Local usage stats; optional anonymous user count and status-bar message, both build-time only

**Install**
* Single self-installing `rideshell.exe` (per user, no admin): PATH, Start menu (normal and
  Administrator), Windows Terminal profile, Explorer menu, Apps & Features. Uninstall is clean.
