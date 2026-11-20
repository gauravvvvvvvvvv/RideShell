# Changelog

Five releases in two days. Newest first.

## Unreleased

* Wrong-account guard: subcommands are matched in order, with options before them skipped,
  so `git log --grep push` or `git commit -m push` no longer ask for confirmation, while
  `git -C repo push` and `gh pr -R owner/repo create` still do. Bare `vercel` (which
  deploys), `docker image push` and `wrangler versions deploy` / `pages deploy` are guarded
  too, also with config files written by older versions.
* `rideshell doctor` shows which guarded tools are protected and which installed ones aren't
  routed through RideShell (so their sensitive commands run unchecked).
* Tool shims pass arguments through byte for byte: a file name that isn't valid Unicode no
  longer stops `git add` and friends on Linux.
* Updates: an update is refused unless it is newer than the installed version, so an old
  signed release can't be pushed as a downgrade.
* RideShell App uses less CPU: terminal output that arrives together is drawn in one go,
  the Spotify watcher keeps one connection to Windows, and the background refresh runs
  every minute instead of every 10 seconds (focusing the window still refreshes at once)
  and redraws nothing when nothing changed. Idle CPU went from about 2.5% to 0.5% of a core.
* CI: formatting, Clippy, tests and release builds on Windows and Linux.

## 0.1.4 (2026-11-20)

**Clickable paths, a tidier sidebar, a proper Apps entry.**

* Click any file or folder path in a tab to open it in Explorer (file selected). Works for
  `C:\…`, quoted paths with spaces, `~\…`, `%VAR%\…`, `/c/…` and `src/main.rs:12:5`, even
  when wrapped over two rows.
* Each account's tools are now a row of small icons; click one for its options, **+** to
  connect another.
* *Settings → Apps* / Control Panel shows RideShell's size, install date and links.
* README, architecture and roadmap rewritten to cover everything so far.

## 0.1.3 (2026-11-20)

**Signed updates and a wrong-account guard.**

* Updates install only with a valid Ed25519 release signature over the checksum list. A
  tampered release or a taken-over GitHub account can't push one.
* Before `git push`, `gh pr create`, deploys and publishes as the wrong account for a folder,
  RideShell asks (`[guard]`, `RIDESHELL_GUARD=0` to skip).
* Linux: keys verified in a real GNOME Keyring.

## 0.1.2 (2026-11-20)

**Updates inside the app, an install wizard, and Linux.**

* RideShell App offers new versions in a popup and installs them in a wizard, with no
  terminal window.
* Double-clicking `rideshell.exe` opens an install wizard instead of a console.
* The `rideshell` CLI ships for Linux x86-64.

## 0.1.1 (2026-11-20)

**Safer switching, private history, no sign-in clutter.**

* Account switches wait for the shell to confirm, and never type into a running program.
* New accounts keep their conversation history private (sharing is opt-in).
* Removed `rideshell auth` and the Sign in / API key buttons; tools sign in by themselves.
* Tools you install while the app is open appear without a restart.
* Slow work (scans, folder listings, input) moved off the window's thread.

## 0.1.0 (2026-11-20)

**First release: every account, every shell, one terminal.**

* Separate Claude Code, Codex, GitHub CLI and Git accounts per profile, side by side.
* Folders bound to accounts; secrets in the OS credential store; tool shims.
* RideShell window: PowerShell, cmd and bash lines in one session.
* RideShell App: tabs, accounts sidebar, launchers, files panel, command palette, themes,
  Spotify controls, Administrator windows.
* Profiles apply only inside RideShell; plain terminals are untouched.
* One self-installing `rideshell.exe` (per user, no admin) with a clean uninstall.
