# Changelog

## 0.1.3 (2026-11-20)

* **Signed updates.** Releases now publish `SHA256SUMS` and an Ed25519 signature
  (`SHA256SUMS.sig`) made with the project's offline release key. RideShell App installs an
  update only when the signature is valid and the download matches the signed list. A tampered
  release or a taken-over GitHub account can't push an update without the key. Maintainers sign
  with the new `rideshell-release` tool (not shipped).
* **Wrong-account guard.** Before `git push`, `gh pr create`, deploys, publishes and similar
  commands, RideShell asks when the terminal's account differs from the folder's bound account.
  Without a terminal to ask in, it refuses (`RIDESHELL_GUARD=0` overrides). Configurable in
  `[guard]`. Connecting a Git identity now routes `git` through RideShell so `git push` is covered
  (outside RideShell, `git` runs untouched).
* **Linux keyring tested:** keys stored in and read from a real GNOME Keyring over D-Bus, kept
  apart per account, removed with the account.
* Docs: `SECURITY.md` lists the update check and what the signature proves; the release process
  is in `CONTRIBUTING.md` (and its broken build-script path is fixed).

## 0.1.2 (2026-11-20)

* **Updates inside the app.** RideShell App checks GitHub Releases shortly after start and every
  6 hours, and offers a new version in a popup (Install update / Later / Skip this version). The
  update installs in a wizard (download, SHA-256 check against the release's checksum file,
  install with no terminal window, restart). *Settings → Updates* turns the check off or runs it
  now.
* **Install wizard.** Double-clicking the downloaded `rideshell.exe` opens an install window
  instead of a console.
* **Linux:** the `rideshell` command-line tool is published for Linux (x86-64). Profiles, folder
  bindings, bash/zsh integration, tool shims and secrets (Secret Service). RideShell App is
  Windows-only for now.

## 0.1.1 (2026-11-20)

* Conversation history of new Claude Code / Codex accounts is **private by default** again, so
  work and personal never mix. Share it per account (`rideshell history share`, or the tool's ⋯
  menu), or for every new account with `[history] share_new_accounts = true` (Settings → Profiles).
  Creating or connecting an account says how.
* App: switching accounts only happens at an idle prompt, and the account shown changes only once
  the shell confirms it; otherwise the tab says it could not switch.
* App: folder listings, account creation and keystrokes no longer run on the window's thread, so
  a slow folder or a program that stops reading input can't freeze the window.
* App: labels cut off with "…" show their full text on hover.
* Removed `rideshell auth` (login / logout / status) and the private-browser sign-in helper. Tools
  sign in by themselves: run the tool in a tab of the account, and its login is saved there.
  `connect` and custom tools no longer ask for or store a sign-in command.
* App: account rows no longer have Sign in / API key buttons. Each tool signs in by itself when
  you run it in a profile, and keys are that profile's own variables (`rideshell secret set`).
* App: tools installed while the app is open (winget, setups) appear in Launch and "All installed
  tools" without a restart; new tabs get the updated PATH too.

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
* Tool shims: `claude`, `codex`, `gh` and any `shim add <tool>` use the right account in RideShell terminals

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
