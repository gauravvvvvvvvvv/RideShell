# Security

RideShell sits between developers and their accounts, so its first duty is never to make
credential handling worse.

## What RideShell stores

* **Profiles, plain variables, identity settings, folder bindings, project variables**, in a
  local SQLite database in your user data directory (`rideshell config path`). Identity
  settings are references: an account name, a config-folder path, an e-mail address.
* **Secrets (API keys, tokens)** go in the **operating system's credential store**:
  Windows Credential Manager, macOS Keychain, or Linux Secret Service, under the service
  name `rideshell`. The database records only the secret's *name*. Values are never
  written to the database, logs, exports or `RIDESHELL_STATE`.
* **Per-account tool folders** (`accounts/<tool>/<account>`) are where tools such as Claude
  Code and Codex keep *their own* logins for that account. RideShell creates the folder and
  points the tool at it, but never reads, copies or moves the tokens inside.
* Plain variables whose name or value looks like a credential are refused and redirected to
  `rideshell secret set`. This heuristic catches common mistakes. It is not a guarantee.

## Network

The `rideshell` command-line tool makes no network requests. RideShell App makes only the ones
below, and each only happens in the case described:

* **Tools you run** (Claude Code, git, npm...) use the network as they always do.
* **Update check (RideShell App, Windows):** shortly after start and every 6 hours, while
  *Settings → Updates → Check for updates automatically* is on (the default), one HTTPS GET to
  `api.github.com/repos/gauravvvvvvvvvv/RideShell/releases/latest`, with a generic
  `RideShell-updater/<version>` User-Agent and no ID or cookies. It runs through Windows' own
  `curl.exe`, hidden. Nothing downloads or installs unless you choose **Install update**. Then
  the release's `rideshell.exe`, `SHA256SUMS` and `SHA256SUMS.sig` are downloaded, only from
  this repository's release URLs and only over HTTPS.
  * **Signed updates (from 0.1.3):** `SHA256SUMS` must carry a valid **Ed25519 signature**
    made with the project's release key, and the download's SHA-256 must be the one it lists.
    Otherwise nothing is installed. The release key is kept offline by the maintainer; its
    public half is built into RideShell (`crates/rideshell-core/src/release.rs`). So a tampered
    release, or someone who takes over the GitHub account or repository without the key, cannot
    get an update installed. (RideShell 0.1.2 checked only an unsigned `.sha256` file from the
    same release; it updates to a signed version once, after which updates must be signed.)
  * The executables are **not code-signed** (Authenticode), so Windows SmartScreen may warn
    on first download.
* **Anonymous user count:** only in builds made with `RIDESHELL_TELEMETRY_URL`, and only
  while it is enabled (Settings → Privacy & usage; `DO_NOT_TRACK=1` and
  `RIDESHELL_NO_TELEMETRY=1` turn it off). At most every 5 minutes, RideShell App sends
  `{id, v, os, m}`: a random ID that changes every UTC day, the version, the OS family and
  active minutes. There are no cookies and no referrer. The reference server
  ([`server/`](server/)) stores no IP addresses.
* **Status-bar message:** only in builds made with `RIDESHELL_PROMO_URL`. RideShell App
  fetches one short JSON file, with no ID, cookies or referrer. The text is shown as plain
  text, and links open only `https://` URLs, in your browser.
* **Program icons** are read from local executables, not downloaded.

Builds from source have neither the counter nor the message slot unless you set those
variables.

## Where secrets go at runtime

* Secrets are read from the credential store only when a program starts through a **shim** or
  `rideshell run`, and are placed in **that process's environment only**.
* The prompt hook and `rideshell env --shell` never output secrets, so they are never
  exported into an interactive shell.
* As with any environment variable, the launched program and its children can read it. That
  is how tools receive API keys.

## What RideShell changes

* **Install:** the program folder, the *user* PATH (two entries added to the front), one
  Apps & Features entry under HKCU, two Start menu shortcuts, a Windows Terminal profile
  fragment, and the per-user "Open in RideShell" folder menu entry in Explorer
  (HKCU\Software\Classes). `uninstall --program` reverses all of them. Installing never needs or asks for
  admin rights.
* **Administrator windows** are opened only when you ask (`rideshell admin`, the
  *(Administrator)* shortcut), always through the normal Windows UAC prompt. RideShell never
  elevates silently, keeps no elevated background process, and stores nothing extra for
  admin use.
* **Shell startup files:** only via `rideshell shell install` / `init`, after showing the
  exact block. It is reversible, and a backup is made first.
* **Never:** git config, SSH config, your tools' default logins (`~/.claude`, `~/.codex`, ...),
  or project files.

## Failure behaviour

* If the selected profile does not exist (deleted or renamed while a terminal still refers
  to it), RideShell activates **no** profile, and shims **refuse to start the tool**, rather
  than fall back to another account.
* The prompt hook never evaluates output unless it starts with RideShell's emit marker.
* All emitted shell code is quoted for its shell (POSIX single quotes; PowerShell ASCII-only
  literals), so values cannot inject commands.

## Command routing in RideShell windows

Lines routed to cmd or bash run with the same rights as the window. The line is passed through
an environment variable and a temporary file in your temp folder, which is deleted after use.
State brought back from cmd/bash is applied as generated, quoted PowerShell code
(ASCII-only literals), never by evaluating the shell's raw output.

## Known limitations

* Environment variables are visible to every process the shell starts, and to the same
  user through OS tooling. This is inherent to environment-based selection and matches how
  the underlying tools expect to be configured.
* `RIDESHELL_STATE` records the *original* values of variables RideShell overrode, so it can
  restore them (it stores only fingerprints of the values RideShell set). If you had exported
  a secret yourself in a variable a profile overrides, that original value is also present
  in `RIDESHELL_STATE` in the same process environment.
* Anyone who can run programs as your OS user can read your credential store entries, as
  with every tool that uses it.

## Reporting a vulnerability

Please **do not open a public issue** for security problems. Use GitHub's private
vulnerability reporting ("Report a vulnerability" under the repository's Security tab).
Include the version (`rideshell --version`), OS, shell, and reproduction steps. We aim to
acknowledge reports within a week. Fixes are released as soon as practical, with credit if
you want it.
