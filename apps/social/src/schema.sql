-- Social Space App schema.

CREATE TABLE IF NOT EXISTS app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL DEFAULT ''
);

-- One connected social account. `official_config` is JSON holding whatever the
-- platform's official API needs (app_key/app_secret/access_token/page_id/...).
-- The extension-captured session tokens are NOT stored here — they live in the
-- extension and never cross the app boundary. `session_present` is only a flag.
CREATE TABLE IF NOT EXISTS accounts (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    platform        TEXT NOT NULL,            -- facebook|tiktok|x|instagram|youtube
    handle          TEXT NOT NULL DEFAULT '', -- @username / page name
    display_name    TEXT NOT NULL DEFAULT '',
    official_config TEXT NOT NULL DEFAULT '{}',
    session_present INTEGER NOT NULL DEFAULT 0,
    token_expiry    TEXT NOT NULL DEFAULT '',
    enabled         INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE(platform, handle)
);

-- Inbound/outbound messages captured from a platform inbox.
CREATE TABLE IF NOT EXISTS inbox (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id  INTEGER,
    platform    TEXT NOT NULL,
    thread_id   TEXT NOT NULL DEFAULT '',
    external_id TEXT NOT NULL DEFAULT '',
    sender      TEXT NOT NULL DEFAULT '',     -- display name of the counterpart (for inbound)
    direction   TEXT NOT NULL,               -- in|out
    text        TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL
);

-- Every posting attempt.
CREATE TABLE IF NOT EXISTS post_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER,
    platform   TEXT NOT NULL,
    kind       TEXT NOT NULL DEFAULT 'post', -- post|video|reply
    ref_id     TEXT NOT NULL DEFAULT '',
    status     TEXT NOT NULL DEFAULT 'pending',
    detail     TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

-- Autonomy gate: every write (post/reply) becomes a draft first unless the app
-- is in "live" mode. A human approves a draft before it goes out.
CREATE TABLE IF NOT EXISTS drafts (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    platform   TEXT NOT NULL,
    handle     TEXT NOT NULL DEFAULT '',
    kind       TEXT NOT NULL,               -- post|reply
    text       TEXT NOT NULL DEFAULT '',
    thread_id  TEXT NOT NULL DEFAULT '',     -- for kind=reply
    status     TEXT NOT NULL DEFAULT 'pending', -- pending|sent|rejected
    ref_id     TEXT NOT NULL DEFAULT '',     -- platform id once sent
    detail     TEXT NOT NULL DEFAULT '',     -- error/result note
    media      TEXT NOT NULL DEFAULT '[]',   -- JSON array of image data URLs
    created_at TEXT NOT NULL,
    decided_at TEXT NOT NULL DEFAULT ''
);

-- Login/session history: one row each time a platform session appears or
-- disappears (derived from the extension heartbeat's hosts_ready transitions).
CREATE TABLE IF NOT EXISTS session_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    platform   TEXT NOT NULL,
    event      TEXT NOT NULL,               -- online|offline
    created_at TEXT NOT NULL
);

-- Audit trail for the cadence governor: one row per reserved action.
CREATE TABLE IF NOT EXISTS action_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    platform   TEXT NOT NULL,
    account_id INTEGER,
    action     TEXT NOT NULL,                -- post|search|feed|dm|inbox|groups
    status     TEXT NOT NULL,               -- reserved|blocked
    detail     TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
