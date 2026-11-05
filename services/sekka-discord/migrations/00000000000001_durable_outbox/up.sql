CREATE TABLE store_metadata (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    channel TEXT NOT NULL,
    activation_at BIGINT NOT NULL,
    next_poll_after BIGINT NOT NULL DEFAULT 0,
    last_backfill_at BIGINT
);

CREATE TABLE notifications (
    key TEXT PRIMARY KEY NOT NULL,
    occurred_at BIGINT NOT NULL,
    message TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'sending', 'sent', 'suppressed')),
    received_at BIGINT NOT NULL,
    attempted_at BIGINT,
    sent_at BIGINT,
    discord_message_id TEXT,
    CHECK (state NOT IN ('sending', 'sent') OR attempted_at IS NOT NULL),
    CHECK ((state = 'sent') = (sent_at IS NOT NULL AND discord_message_id IS NOT NULL))
);

CREATE INDEX notifications_dispatch ON notifications (state, received_at, key);

CREATE TABLE receipts (
    source TEXT NOT NULL,
    delivery_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    fingerprint TEXT,
    notification_key TEXT REFERENCES notifications (key),
    received_at BIGINT NOT NULL,
    legacy_disposition TEXT CHECK (legacy_disposition IN ('seen', 'pending')),
    PRIMARY KEY (source, delivery_id)
);

CREATE INDEX receipts_notification ON receipts (notification_key);

-- Keep old outbox identities resolvable after their Events API semantic mapping.
CREATE TABLE notification_aliases (
    key TEXT PRIMARY KEY NOT NULL,
    notification_key TEXT NOT NULL REFERENCES notifications (key)
);
