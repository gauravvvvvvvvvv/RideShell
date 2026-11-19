# RideShell usage counter (optional)

A tiny Cloudflare Worker + D1 database that counts **anonymous** RideShell App usage:
how many people use it right now, per day, and for how long. The owner dashboard is at
`/`.

It only receives data from builds made with `RIDESHELL_TELEMETRY_URL` set (see
[Building with the counter](#building-with-the-counter)). Builds without it, including
every fork built with the defaults, contain no counter at all.

## What is sent, exactly

While RideShell App is open, at most once every 5 minutes:

```json
{ "id": "3f9c…(32 hex)", "v": "0.1.0", "os": "windows", "m": 5 }
```

| Field | Meaning |
|---|---|
| `id` | Random, generated on the user's PC, **replaced every UTC day**. Days cannot be linked. |
| `v` | RideShell version |
| `os` | `windows`, `macos` or `linux` |
| `m` | Minutes the window was in use since the last ping (0–10) |

Nothing else exists to be sent: no commands, folders, file names, accounts, tool names,
user names or settings. The Worker stores no IP addresses and logs no headers. Requests
are sent without cookies or referrer.

Users turn it off in **Settings → Privacy & usage**, or with `DO_NOT_TRACK=1` /
`RIDESHELL_NO_TELEMETRY=1`. The request runs in the background of the app: no window and no
message. Settings → Privacy & usage shows exactly what is sent.

## Deploy

```sh
cd server
npx wrangler d1 create rideshell-telemetry      # put the printed id into wrangler.toml
npx wrangler d1 execute rideshell-telemetry --remote --file=schema.sql
npx wrangler secret put STATS_TOKEN              # a long random string, for the dashboard
npx wrangler deploy
```

Open the Worker's URL to see the dashboard and enter the token. It shows live now (pings in the
last 10 minutes), users today, average minutes, a 30-day chart, versions and OS.

**Public badges** (totals only, nothing per user): `/badge/live.svg` (live now) and
`/badge/today.svg` (users today), refreshed every minute. To show them under the logo in the
project README, replace `YOUR-WORKER` in the badge lines at the top of `README.md` with your
Worker's address, and remove the comment markers around them.

## Building with the counter

```powershell
$env:RIDESHELL_TELEMETRY_URL = "https://rideshell-telemetry.<you>.workers.dev"
powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1
```

The URL is compiled in. Without it the app has no endpoint and sends nothing.
