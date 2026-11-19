-- RideShell schema v2: secret variables and directory (project) variables.
-- Secret values are NEVER stored here: a row with secret = 1 has an empty
-- value and the real value lives in the operating system's credential store
-- (Windows Credential Manager / macOS Keychain / Secret Service).

ALTER TABLE profile_env ADD COLUMN secret INTEGER NOT NULL DEFAULT 0;

-- Variables attached to a directory (and everything below it), independent
-- of the active profile. Deeper directories override shallower ones.
CREATE TABLE dir_env (
    id       INTEGER PRIMARY KEY,
    path     TEXT    NOT NULL,
    path_key TEXT    NOT NULL,
    key      TEXT    NOT NULL COLLATE NOCASE,
    value    TEXT    NOT NULL,
    secret   INTEGER NOT NULL DEFAULT 0,
    UNIQUE (path_key, key)
);
