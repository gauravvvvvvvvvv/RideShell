-- RideShell schema v1.
-- Secrets are never stored here: env values and identity settings hold
-- references (account names, profile names, file paths) only.

CREATE TABLE profiles (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    description TEXT    NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- Plain environment variables applied when the profile is active.
CREATE TABLE profile_env (
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    key        TEXT    NOT NULL COLLATE NOCASE,
    value      TEXT    NOT NULL,
    PRIMARY KEY (profile_id, key)
);

-- Per-provider identity settings (JSON object of string -> string).
CREATE TABLE identities (
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    provider   TEXT    NOT NULL,
    settings   TEXT    NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (profile_id, provider)
);

-- Directory -> profile bindings. path_key is the normalised match key.
CREATE TABLE bindings (
    id         INTEGER PRIMARY KEY,
    path       TEXT    NOT NULL,
    path_key   TEXT    NOT NULL UNIQUE,
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
);
CREATE INDEX bindings_profile ON bindings(profile_id);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
