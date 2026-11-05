use crate::events::{Notification, stamp_message};
use anyhow::{Context, Result, bail, ensure};
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
const EVENTS_SOURCE: &str = "events_api";

diesel::table! {
    store_metadata (singleton) {
        singleton -> Integer,
        channel -> Text,
        activation_at -> BigInt,
        next_poll_after -> BigInt,
        last_backfill_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    notifications (key) {
        key -> Text,
        occurred_at -> BigInt,
        message -> Text,
        state -> Text,
        received_at -> BigInt,
        attempted_at -> Nullable<BigInt>,
        sent_at -> Nullable<BigInt>,
        discord_message_id -> Nullable<Text>,
    }
}

diesel::table! {
    receipts (source, delivery_id) {
        source -> Text,
        delivery_id -> Text,
        kind -> Text,
        fingerprint -> Nullable<Text>,
        notification_key -> Nullable<Text>,
        received_at -> BigInt,
        legacy_disposition -> Nullable<Text>,
    }
}

diesel::table! {
    notification_aliases (key) {
        key -> Text,
        notification_key -> Text,
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = receipts)]
struct Receipt {
    fingerprint: Option<String>,
    kind: String,
    notification_key: Option<String>,
    legacy_disposition: Option<String>,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = notifications)]
struct StoredNotification {
    key: String,
    message: String,
    state: String,
    received_at: i64,
    attempted_at: Option<i64>,
    sent_at: Option<i64>,
    discord_message_id: Option<String>,
}

impl StoredNotification {
    fn outbox(self) -> Result<Outbox> {
        Ok(Outbox {
            key: self.key,
            message: serde_json::from_str(&self.message)
                .context("Reading stored Discord message")?,
            attempted_at: self
                .attempted_at
                .context("Sending notification has no attempt time")?,
        })
    }
}

#[derive(Debug)]
pub struct Outbox {
    pub key: String,
    pub message: Value,
    pub attempted_at: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum IngestOutcome {
    Queued,
    Duplicate,
    Ignored,
}

pub struct Store {
    connection: SqliteConnection,
    activation_at: i64,
    // Use exactly the old JSON store's lock, excluding old and new binaries alike.
    _lock: File,
}

impl Store {
    pub fn open(dir: &Path, channel: &str, activation_at: i64) -> Result<Self> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .context("Creating state directory")?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(dir.join("lock"))
            .context("Opening state lock")?;
        lock.try_lock()
            .context("Another sekka-discord process may own the state directory")?;

        let database = dir.join("state.sqlite3");
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&database)
            .context("Creating SQLite state file")?;
        let mut connection =
            SqliteConnection::establish(database.to_str().context("State path is not UTF-8")?)
                .context("Opening SQLite state")?;
        connection
            .batch_execute(
                "PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON; \
             PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;",
            )
            .context("Configuring durable SQLite transactions")?;
        connection
            .run_pending_migrations(MIGRATIONS)
            .map_err(|error| anyhow::anyhow!("Applying SQLite migrations: {error}"))?;

        let activation_at =
            connection.immediate_transaction::<_, anyhow::Error, _>(|connection| {
                let existing = store_metadata::table
                    .find(1)
                    .select((store_metadata::channel, store_metadata::activation_at))
                    .first::<(String, i64)>(connection)
                    .optional()?;
                if let Some((bound_channel, original_activation)) = existing {
                    ensure!(
                        bound_channel == channel,
                        "State directory is bound to Discord channel {bound_channel}, not {channel}"
                    );
                    return Ok(original_activation);
                }
                // Metadata and all legacy rows commit together. Once metadata exists,
                // state.json is only a backup, even if subsequently changed or corrupt.
                let legacy = read_legacy(dir)?;
                let next_poll_after = legacy.as_ref().map_or(0, |state| state.next_poll_after);
                diesel::insert_into(store_metadata::table)
                    .values((
                        store_metadata::singleton.eq(1),
                        store_metadata::channel.eq(channel),
                        store_metadata::activation_at.eq(activation_at),
                        store_metadata::next_poll_after.eq(next_poll_after),
                    ))
                    .execute(connection)?;
                if let Some(legacy) = legacy {
                    import_legacy(connection, legacy, activation_at)?;
                }
                Ok(activation_at)
            })?;
        File::open(dir)?
            .sync_all()
            .context("Syncing SQLite state directory")?;
        Ok(Self {
            connection,
            activation_at,
            _lock: lock,
        })
    }

    pub fn ingest(
        &mut self,
        source: &str,
        delivery_id: &str,
        kind: &str,
        fingerprint: &str,
        notification: Option<&Notification>,
        now: i64,
    ) -> Result<IngestOutcome> {
        let activation_at = self.activation_at;
        self.connection.immediate_transaction(|connection| {
            let previous = receipts::table
                .find((source, delivery_id))
                .select(Receipt::as_select())
                .first::<Receipt>(connection)
                .optional()?;
            if let Some(previous) = &previous {
                let attaching_legacy_fingerprint = previous.legacy_disposition.is_some()
                    && previous.fingerprint.as_deref().is_none_or(str::is_empty);
                if !attaching_legacy_fingerprint {
                    ensure!(
                        previous.fingerprint.as_deref() == Some(fingerprint)
                            && previous.kind == kind,
                        "Receipt collision for {source}/{delivery_id}"
                    );
                }
                if previous.legacy_disposition.is_none() {
                    return Ok(IngestOutcome::Duplicate);
                }
            }

            let mut outcome = IngestOutcome::Ignored;
            if let Some(notification) = notification {
                let legacy_seen = previous
                    .as_ref()
                    .and_then(|receipt| receipt.legacy_disposition.as_deref())
                    == Some("seen");
                let suppressed = legacy_seen || notification.occurred_at < activation_at;
                let inserted = insert_notification(connection, notification, now, suppressed)?;
                if suppressed {
                    // A webhook may already have arrived. Seen or historical
                    // semantic events must not dispatch after startup backfill.
                    diesel::update(
                        notifications::table
                            .find(&notification.key)
                            .filter(notifications::state.eq("pending")),
                    )
                    .set(notifications::state.eq("suppressed"))
                    .execute(connection)?;
                }
                if let Some(previous) = &previous {
                    if previous.legacy_disposition.as_deref() == Some("pending") {
                        let old_key = previous
                            .notification_key
                            .as_deref()
                            .context("Legacy pending receipt has no outbox key")?;
                        merge_legacy(connection, old_key, &notification.key, inserted)?;
                    }
                }
                let state = notifications::table
                    .find(&notification.key)
                    .select(notifications::state)
                    .first::<String>(connection)?;
                outcome = if state == "suppressed" {
                    IngestOutcome::Ignored
                } else if inserted && state == "pending" {
                    IngestOutcome::Queued
                } else {
                    IngestOutcome::Duplicate
                };
            }

            if previous.is_some() {
                // A legacy ignored event can remain unmapped. A later available
                // normalization must still be able to resolve its legacy identity.
                if let Some(notification) = notification {
                    diesel::update(receipts::table.find((source, delivery_id)))
                        .set((
                            receipts::kind.eq(kind),
                            receipts::fingerprint.eq(Some(fingerprint)),
                            receipts::notification_key.eq(Some(&notification.key)),
                            receipts::legacy_disposition.eq(None::<&str>),
                        ))
                        .execute(connection)?;
                } else {
                    diesel::update(receipts::table.find((source, delivery_id)))
                        .set((
                            receipts::kind.eq(kind),
                            receipts::fingerprint.eq(Some(fingerprint)),
                        ))
                        .execute(connection)?;
                }
            } else {
                diesel::insert_into(receipts::table)
                    .values((
                        receipts::source.eq(source),
                        receipts::delivery_id.eq(delivery_id),
                        receipts::kind.eq(kind),
                        receipts::fingerprint.eq(Some(fingerprint)),
                        receipts::notification_key.eq(notification.map(|value| value.key.as_str())),
                        receipts::received_at.eq(now),
                    ))
                    .execute(connection)?;
            }
            Ok(outcome)
        })
    }

    pub fn claim_next(&mut self, now: i64) -> Result<Option<Outbox>> {
        self.connection.immediate_transaction(|connection| {
            let next = notifications::table
                .filter(notifications::state.eq("pending"))
                .order((notifications::received_at.asc(), notifications::key.asc()))
                .select(StoredNotification::as_select())
                .first::<StoredNotification>(connection)
                .optional()?;
            let Some(mut next) = next else {
                return Ok(None);
            };
            next.attempted_at = Some(now);
            let outbox = next.outbox()?;
            diesel::update(notifications::table.find(&outbox.key))
                .set((
                    notifications::state.eq("sending"),
                    notifications::attempted_at.eq(Some(now)),
                ))
                .execute(connection)?;
            Ok(Some(outbox))
        })
    }

    pub fn inflight(&mut self) -> Result<Vec<Outbox>> {
        notifications::table
            .filter(notifications::state.eq("sending"))
            .order((notifications::attempted_at.asc(), notifications::key.asc()))
            .select(StoredNotification::as_select())
            .load::<StoredNotification>(&mut self.connection)?
            .into_iter()
            .map(StoredNotification::outbox)
            .collect()
    }

    pub fn mark_sent(&mut self, key: &str, discord_message_id: &str, now: i64) -> Result<()> {
        self.connection.immediate_transaction(|connection| {
            let key = resolve_key(connection, key)?;
            let previous = notifications::table
                .find(&key)
                .select(StoredNotification::as_select())
                .first::<StoredNotification>(connection)?;
            if previous.state == "sent" {
                ensure!(
                    previous.discord_message_id.as_deref() == Some(discord_message_id),
                    "Notification {key} already records a different Discord message"
                );
                return Ok(());
            }
            ensure!(
                previous.state == "sending",
                "Notification {key} is not in flight"
            );
            diesel::update(notifications::table.find(&key))
                .set((
                    notifications::state.eq("sent"),
                    notifications::discord_message_id.eq(Some(discord_message_id)),
                    notifications::sent_at.eq(Some(now)),
                ))
                .execute(connection)?;
            Ok(())
        })
    }

    pub fn retry_rejected(&mut self, key: &str) -> Result<()> {
        self.connection.immediate_transaction(|connection| {
            let key = resolve_key(connection, key)?;
            let state = notifications::table
                .find(&key)
                .select(notifications::state)
                .first::<String>(connection)?;
            ensure!(
                state == "sending" || state == "pending",
                "Notification {key} cannot be retried"
            );
            diesel::update(notifications::table.find(&key))
                .set((
                    notifications::state.eq("pending"),
                    notifications::attempted_at.eq(None::<i64>),
                ))
                .execute(connection)?;
            Ok(())
        })
    }

    pub fn finish_backfill(&mut self, now: i64) -> Result<()> {
        diesel::update(store_metadata::table.find(1))
            .set(store_metadata::last_backfill_at.eq(Some(now)))
            .execute(&mut self.connection)?;
        Ok(())
    }

    pub fn has_receipt(&mut self, source: &str, id: &str) -> Result<bool> {
        Ok(
            diesel::select(diesel::dsl::exists(receipts::table.find((source, id))))
                .get_result(&mut self.connection)?,
        )
    }

    pub fn status(&mut self) -> Result<Value> {
        self.connection.transaction(|connection| {
            let (channel, activation_at, next_poll_after, last_backfill_at) = store_metadata::table
                .find(1)
                .select((
                    store_metadata::channel,
                    store_metadata::activation_at,
                    store_metadata::next_poll_after,
                    store_metadata::last_backfill_at,
                ))
                .first::<(String, i64, i64, Option<i64>)>(connection)?;
            let receipt_count = receipts::table.count().get_result::<i64>(connection)?;
            let notification_count = notifications::table.count().get_result::<i64>(connection)?;
            let mut status = json!({
                "receipts": receipt_count,
                "notifications": notification_count,
                "channel": channel,
                "activation_at": activation_at,
                "next_poll_after": next_poll_after,
                "last_backfill_at": last_backfill_at,
            });
            for state in ["pending", "sending", "sent", "suppressed"] {
                status[state] = notifications::table
                    .filter(notifications::state.eq(state))
                    .count()
                    .get_result::<i64>(connection)?
                    .into();
            }
            Ok(status)
        })
    }
}

fn insert_notification(
    connection: &mut SqliteConnection,
    notification: &Notification,
    received_at: i64,
    suppressed: bool,
) -> Result<bool> {
    Ok(diesel::insert_into(notifications::table)
        .values((
            notifications::key.eq(&notification.key),
            notifications::occurred_at.eq(notification.occurred_at),
            notifications::message.eq(serde_json::to_string(&notification.message)?),
            notifications::state.eq(if suppressed { "suppressed" } else { "pending" }),
            notifications::received_at.eq(received_at),
        ))
        .on_conflict(notifications::key)
        .do_nothing()
        .execute(connection)?
        == 1)
}

fn resolve_key(connection: &mut SqliteConnection, key: &str) -> Result<String> {
    Ok(notification_aliases::table
        .find(key)
        .select(notification_aliases::notification_key)
        .first::<String>(connection)
        .optional()?
        .unwrap_or_else(|| key.to_owned()))
}

fn merge_legacy(
    connection: &mut SqliteConnection,
    old_key: &str,
    canonical_key: &str,
    canonical_inserted: bool,
) -> Result<()> {
    if old_key == canonical_key {
        return Ok(());
    }
    let old = notifications::table
        .find(old_key)
        .select(StoredNotification::as_select())
        .first::<StoredNotification>(connection)?;
    let canonical = notifications::table
        .find(canonical_key)
        .select(StoredNotification::as_select())
        .first::<StoredNotification>(connection)?;
    let preserve_attempt = old.state == "sent" && canonical.state != "sent"
        || old.state == "sending" && canonical.state != "sent" && canonical.state != "sending";
    if preserve_attempt {
        // Never rewrite an attempted message's footer/nonce. Recovery must match
        // the marker actually sent, even though its DB identity is now canonical.
        diesel::update(notifications::table.find(canonical_key))
            .set((
                notifications::state.eq(&old.state),
                notifications::message.eq(&old.message),
                notifications::attempted_at.eq(old.attempted_at),
                notifications::sent_at.eq(old.sent_at),
                notifications::discord_message_id.eq(&old.discord_message_id),
            ))
            .execute(connection)?;
    } else if canonical_inserted && canonical.state == "pending" && old.state == "pending" {
        let mut message: Value = serde_json::from_str(&old.message)?;
        stamp_message(&mut message, canonical_key);
        diesel::update(notifications::table.find(canonical_key))
            .set(notifications::message.eq(serde_json::to_string(&message)?))
            .execute(connection)?;
    }
    diesel::update(notifications::table.find(canonical_key))
        .set(notifications::received_at.eq(old.received_at.min(canonical.received_at)))
        .execute(connection)?;
    diesel::update(receipts::table.filter(receipts::notification_key.eq(old_key)))
        .set(receipts::notification_key.eq(Some(canonical_key)))
        .execute(connection)?;
    diesel::update(
        notification_aliases::table.filter(notification_aliases::notification_key.eq(old_key)),
    )
    .set(notification_aliases::notification_key.eq(canonical_key))
    .execute(connection)?;
    diesel::insert_into(notification_aliases::table)
        .values((
            notification_aliases::key.eq(old_key),
            notification_aliases::notification_key.eq(canonical_key),
        ))
        .execute(connection)?;
    diesel::delete(notifications::table.find(old_key)).execute(connection)?;
    Ok(())
}

#[derive(Deserialize)]
struct LegacyState {
    version: u8,
    seen: Vec<String>,
    pending: Vec<LegacyPending>,
    next_poll_after: i64,
}

#[derive(Deserialize)]
struct LegacyPending {
    id: String,
    message: Value,
}

fn read_legacy(dir: &Path) -> Result<Option<LegacyState>> {
    let bytes = match fs::read(dir.join("state.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("Reading legacy checkpoint"),
    };
    let legacy: LegacyState = serde_json::from_slice(&bytes)
        .context("Reading legacy checkpoint; refusing to discard corrupt state")?;
    ensure!(
        legacy.version == 1 && legacy.seen.len() <= 4096 && legacy.next_poll_after >= 0,
        "Unsupported or invalid legacy checkpoint; refusing to reset it"
    );
    let mut ids = HashSet::new();
    for id in legacy
        .seen
        .iter()
        .chain(legacy.pending.iter().map(|item| &item.id))
    {
        if !ids.insert(id) {
            bail!("Duplicate legacy event ID {id}; refusing to discard pending state");
        }
    }
    Ok(Some(legacy))
}

fn import_legacy(connection: &mut SqliteConnection, state: LegacyState, now: i64) -> Result<()> {
    for id in state.seen {
        diesel::insert_into(receipts::table)
            .values((
                receipts::source.eq(EVENTS_SOURCE),
                receipts::delivery_id.eq(id),
                receipts::kind.eq("legacy"),
                receipts::received_at.eq(now),
                receipts::legacy_disposition.eq(Some("seen")),
            ))
            .execute(connection)?;
    }
    for pending in state.pending {
        let key = format!("legacy:{}", pending.id);
        let mut message = pending.message;
        stamp_message(&mut message, &key);
        insert_notification(
            connection,
            &Notification {
                key: key.clone(),
                occurred_at: now,
                message,
            },
            now,
            false,
        )?;
        diesel::insert_into(receipts::table)
            .values((
                receipts::source.eq(EVENTS_SOURCE),
                receipts::delivery_id.eq(pending.id),
                receipts::kind.eq("legacy"),
                receipts::notification_key.eq(Some(key)),
                receipts::received_at.eq(now),
                receipts::legacy_disposition.eq(Some("pending")),
            ))
            .execute(connection)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::marker;
    use parking_lot::Mutex;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "sekka-discord-db-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn open(&self) -> Store {
            Store::open(&self.0, "channel", 100).unwrap()
        }

        fn legacy(&self, seen: &[&str], pending: &[&str]) {
            fs::write(
                self.0.join("state.json"),
                serde_json::to_vec(&json!({
                    "version": 1,
                    "initialized": true,
                    "seen": seen,
                    "pending": pending.iter().map(|id| json!({
                        "id": id,
                        "message": {"embeds": [{"description": format!("legacy {id}")}]},
                    })).collect::<Vec<_>>(),
                    "next_poll_after": 150,
                }))
                .unwrap(),
            )
            .unwrap();
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn notification(key: &str, occurred_at: i64) -> Notification {
        let mut message = json!({"embeds": [{"description": key}]});
        stamp_message(&mut message, key);
        Notification {
            key: key.to_owned(),
            occurred_at,
            message,
        }
    }

    fn ingest(store: &mut Store, source: &str, id: &str, value: &Notification) -> IngestOutcome {
        store
            .ingest(
                source,
                id,
                "push",
                &format!("fingerprint:{id}"),
                Some(value),
                110,
            )
            .unwrap()
    }

    fn row(store: &mut Store, key: &str) -> StoredNotification {
        notifications::table
            .find(key)
            .select(StoredNotification::as_select())
            .first(&mut store.connection)
            .unwrap()
    }

    #[test]
    fn webhook_and_feed_share_one_notification_in_either_order() {
        for webhook_first in [true, false] {
            let dir = TestDirectory::new();
            let mut store = dir.open();
            let value = notification("push:main:before:after", 105);
            let sources = if webhook_first {
                [("webhook", "guid"), (EVENTS_SOURCE, "event")]
            } else {
                [(EVENTS_SOURCE, "event"), ("webhook", "guid")]
            };
            assert_eq!(
                ingest(&mut store, sources[0].0, sources[0].1, &value),
                IngestOutcome::Queued
            );
            assert_eq!(
                ingest(&mut store, sources[1].0, sources[1].1, &value),
                IngestOutcome::Duplicate
            );
            assert_eq!(
                ingest(&mut store, sources[0].0, sources[0].1, &value),
                IngestOutcome::Duplicate
            );
            assert_eq!(store.status().unwrap()["receipts"], 2);
            assert_eq!(store.status().unwrap()["notifications"], 1);
            assert_eq!(store.claim_next(120).unwrap().unwrap().key, value.key);
            assert!(store.claim_next(121).unwrap().is_none());
        }
    }

    #[test]
    fn receipt_conflicts_do_not_mutate_committed_work() {
        let dir = TestDirectory::new();
        let mut store = dir.open();
        let first = notification("first", 101);
        let second = notification("second", 102);
        ingest(&mut store, "webhook", "same-id", &first);
        let error = store
            .ingest(
                "webhook",
                "same-id",
                "push",
                "different",
                Some(&second),
                111,
            )
            .unwrap_err();
        assert!(error.to_string().contains("Receipt collision"));
        assert_eq!(store.status().unwrap()["notifications"], 1);
        assert_eq!(store.claim_next(120).unwrap().unwrap().key, "first");
        // Delivery namespaces are independent, even when their IDs coincide.
        assert_eq!(
            ingest(&mut store, EVENTS_SOURCE, "same-id", &second),
            IngestOutcome::Queued
        );
    }

    #[test]
    fn ignored_receipts_are_durable_and_collision_checked() {
        let dir = TestDirectory::new();
        {
            let mut store = dir.open();
            assert_eq!(
                store
                    .ingest("webhook", "ignored", "ping", "body", None, 110)
                    .unwrap(),
                IngestOutcome::Ignored
            );
        }
        let mut store = dir.open();
        assert!(store.has_receipt("webhook", "ignored").unwrap());
        assert!(!store.has_receipt(EVENTS_SOURCE, "ignored").unwrap());
        assert_eq!(
            store
                .ingest("webhook", "ignored", "ping", "body", None, 120)
                .unwrap(),
            IngestOutcome::Duplicate
        );
        assert!(
            store
                .ingest("webhook", "ignored", "ping", "changed", None, 120)
                .is_err()
        );
        assert_eq!(store.status().unwrap()["notifications"], 0);
    }

    #[test]
    fn receipt_failure_rolls_back_outbox_and_ack_state() {
        let dir = TestDirectory::new();
        let mut store = dir.open();
        store
            .connection
            .batch_execute(
                "CREATE TRIGGER fail_receipt BEFORE INSERT ON receipts BEGIN \
             SELECT RAISE(ABORT, 'injected receipt write failure'); END;",
            )
            .unwrap();
        let value = notification("atomic", 101);
        assert!(
            store
                .ingest("webhook", "failed", "push", "body", Some(&value), 110)
                .is_err()
        );
        assert!(!store.has_receipt("webhook", "failed").unwrap());
        assert_eq!(store.status().unwrap()["notifications"], 0);
        store
            .connection
            .batch_execute("DROP TRIGGER fail_receipt;")
            .unwrap();
        assert_eq!(
            ingest(&mut store, "webhook", "failed", &value),
            IngestOutcome::Queued
        );
    }

    #[test]
    fn restart_retains_pending_ambiguous_and_sent_transitions() {
        let dir = TestDirectory::new();
        {
            let mut store = dir.open();
            ingest(&mut store, "webhook", "one", &notification("one", 101));
            ingest(&mut store, "webhook", "two", &notification("two", 102));
        }
        {
            let mut store = dir.open();
            let first = store.claim_next(120).unwrap().unwrap();
            assert_eq!(first.key, "one");
            // Crash after claim, before either HTTP or a recorded HTTP response.
        }
        {
            let mut store = dir.open();
            let inflight = store.inflight().unwrap();
            assert_eq!(inflight.len(), 1);
            assert_eq!(inflight[0].attempted_at, 120);
            assert_eq!(inflight[0].key, "one");
            assert_eq!(store.claim_next(121).unwrap().unwrap().key, "two");
            assert!(store.claim_next(122).unwrap().is_none());
            // Reconciliation found the message after a lost successful response.
            store.mark_sent("one", "discord-one", 125).unwrap();
            // Only a definitive rejection makes the other message retryable.
            store.retry_rejected("two").unwrap();
        }
        let mut store = dir.open();
        assert!(store.inflight().unwrap().is_empty());
        assert_eq!(
            row(&mut store, "one").discord_message_id.as_deref(),
            Some("discord-one")
        );
        store.mark_sent("one", "discord-one", 126).unwrap();
        assert!(store.mark_sent("one", "different-id", 126).is_err());
        assert!(store.retry_rejected("one").is_err());
        assert_eq!(
            ingest(
                &mut store,
                EVENTS_SOURCE,
                "feed-one",
                &notification("one", 101)
            ),
            IngestOutcome::Duplicate
        );
        let second = store.claim_next(130).unwrap().unwrap();
        assert_eq!(second.key, "two");
        assert_eq!(second.attempted_at, 130);
        store.mark_sent("two", "discord-two", 131).unwrap();
        assert!(store.claim_next(132).unwrap().is_none());
        assert_eq!(store.status().unwrap()["sent"], 2);
    }

    #[test]
    fn concurrent_requests_and_claims_are_atomic() {
        let dir = TestDirectory::new();
        let store = Arc::new(Mutex::new(dir.open()));
        std::thread::scope(|scope| {
            for n in 0..8 {
                let store = Arc::clone(&store);
                scope.spawn(move || {
                    let value = notification("shared", 101);
                    let mut store = store.lock();
                    ingest(&mut store, "webhook", &format!("delivery-{n}"), &value);
                    ingest(&mut store, EVENTS_SOURCE, "same-replay", &value);
                });
            }
        });
        assert_eq!(store.lock().status().unwrap()["receipts"], 9);
        let claimed = AtomicU64::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let store = Arc::clone(&store);
                let claimed = &claimed;
                scope.spawn(move || {
                    if store.lock().claim_next(120).unwrap().is_some() {
                        claimed.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });
        assert_eq!(claimed.load(Ordering::Relaxed), 1);
        assert_eq!(store.lock().inflight().unwrap().len(), 1);
    }

    #[test]
    fn activation_is_immutable_and_downtime_events_are_not_baselined() {
        let dir = TestDirectory::new();
        {
            let mut store = dir.open();
            assert_eq!(
                ingest(
                    &mut store,
                    "webhook",
                    "old",
                    &notification("historical", 99)
                ),
                IngestOutcome::Ignored
            );
            store.finish_backfill(110).unwrap();
        }
        let mut store = Store::open(&dir.0, "channel", 500).unwrap();
        assert_eq!(store.status().unwrap()["activation_at"], 100);
        assert_eq!(store.status().unwrap()["last_backfill_at"], 110);
        assert_eq!(
            ingest(
                &mut store,
                EVENTS_SOURCE,
                "boundary",
                &notification("boundary", 100)
            ),
            IngestOutcome::Queued
        );
        assert_eq!(
            ingest(
                &mut store,
                EVENTS_SOURCE,
                "downtime",
                &notification("downtime", 200)
            ),
            IngestOutcome::Queued
        );
        assert_eq!(
            ingest(
                &mut store,
                EVENTS_SOURCE,
                "old-feed",
                &notification("historical", 99)
            ),
            IngestOutcome::Ignored
        );
        assert_eq!(store.status().unwrap()["suppressed"], 1);
        assert_eq!(store.status().unwrap()["pending"], 2);
    }

    #[test]
    fn lock_excludes_old_binary_and_channel_binding_survives_restart() {
        let dir = TestDirectory::new();
        let store = dir.open();
        let old_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.0.join("lock"))
            .unwrap();
        assert!(old_lock.try_lock().is_err());
        assert!(Store::open(&dir.0, "channel", 100).is_err());
        drop(store);
        old_lock.try_lock().unwrap();
        assert!(Store::open(&dir.0, "channel", 100).is_err());
        drop(old_lock);
        assert!(Store::open(&dir.0, "other-channel", 100).is_err());
        assert_eq!(dir.open().status().unwrap()["channel"], "channel");
    }

    #[test]
    fn legacy_seen_suppresses_semantic_notifications_in_both_arrival_orders() {
        for webhook_first in [true, false] {
            let dir = TestDirectory::new();
            dir.legacy(&["old-event"], &[]);
            let value = notification("semantic", 101);
            let mut store = dir.open();
            assert!(store.has_receipt(EVENTS_SOURCE, "old-event").unwrap());
            if webhook_first {
                assert_eq!(
                    ingest(&mut store, "webhook", "guid", &value),
                    IngestOutcome::Queued
                );
            }
            assert_eq!(
                ingest(&mut store, EVENTS_SOURCE, "old-event", &value),
                IngestOutcome::Ignored
            );
            if !webhook_first {
                assert_eq!(
                    ingest(&mut store, "webhook", "guid", &value),
                    IngestOutcome::Ignored
                );
            }
            assert!(store.claim_next(120).unwrap().is_none());
            assert_eq!(store.status().unwrap()["notifications"], 1);
            assert_eq!(store.status().unwrap()["suppressed"], 1);
            assert!(
                store
                    .ingest(
                        EVENTS_SOURCE,
                        "old-event",
                        "push",
                        "changed",
                        Some(&value),
                        120
                    )
                    .is_err()
            );
        }
    }

    #[test]
    fn legacy_pending_merges_without_losing_unmapped_messages() {
        for webhook_first in [true, false] {
            let dir = TestDirectory::new();
            dir.legacy(&[], &["mapped", "expired-from-feed"]);
            let value = notification("canonical", 101);
            let mut store = dir.open();
            if webhook_first {
                ingest(&mut store, "webhook", "guid", &value);
            }
            ingest(&mut store, EVENTS_SOURCE, "mapped", &value);
            if !webhook_first {
                ingest(&mut store, "webhook", "guid", &value);
            }
            assert_eq!(store.status().unwrap()["pending"], 2);
            assert_eq!(store.status().unwrap()["notifications"], 2);
            let canonical = store.claim_next(120).unwrap().unwrap();
            assert_eq!(canonical.key, "canonical");
            assert!(canonical.message.to_string().contains(&marker("canonical")));
            assert!(
                !canonical
                    .message
                    .to_string()
                    .contains(&marker("legacy:mapped"))
            );
            if !webhook_first {
                assert_eq!(
                    canonical.message["embeds"][0]["description"],
                    "legacy mapped"
                );
            }
            let retained = store.claim_next(121).unwrap().unwrap();
            assert_eq!(retained.key, "legacy:expired-from-feed");
            assert_eq!(
                retained.message["embeds"][0]["description"],
                "legacy expired-from-feed"
            );
            assert!(
                retained
                    .message
                    .to_string()
                    .contains(&marker(&retained.key))
            );
            assert!(store.claim_next(122).unwrap().is_none());
        }
    }

    #[test]
    fn legacy_inflight_merge_preserves_original_marker_and_alias() {
        let dir = TestDirectory::new();
        dir.legacy(&[], &["mapped"]);
        let original_message;
        {
            let mut store = dir.open();
            let original = store.claim_next(110).unwrap().unwrap();
            assert_eq!(original.key, "legacy:mapped");
            original_message = original.message;
        }
        let mut store = dir.open();
        let value = notification("canonical", 101);
        ingest(&mut store, "webhook", "guid", &value);
        ingest(&mut store, EVENTS_SOURCE, "mapped", &value);
        assert!(store.claim_next(120).unwrap().is_none());
        let inflight = store.inflight().unwrap();
        assert_eq!(inflight.len(), 1);
        assert_eq!(inflight[0].key, "canonical");
        assert_eq!(inflight[0].attempted_at, 110);
        assert_eq!(inflight[0].message, original_message);
        store
            .mark_sent("legacy:mapped", "discord-message", 130)
            .unwrap();
        assert_eq!(
            row(&mut store, "canonical").discord_message_id.as_deref(),
            Some("discord-message")
        );
    }

    #[test]
    fn legacy_sent_merge_never_resends_and_keeps_message_id() {
        let dir = TestDirectory::new();
        dir.legacy(&[], &["mapped"]);
        {
            let mut store = dir.open();
            store.claim_next(110).unwrap().unwrap();
            store
                .mark_sent("legacy:mapped", "already-sent", 111)
                .unwrap();
        }
        let mut store = dir.open();
        let value = notification("canonical", 101);
        ingest(&mut store, "webhook", "guid", &value);
        assert_eq!(
            ingest(&mut store, EVENTS_SOURCE, "mapped", &value),
            IngestOutcome::Duplicate
        );
        assert_eq!(store.status().unwrap()["sent"], 1);
        assert_eq!(
            row(&mut store, "canonical").discord_message_id.as_deref(),
            Some("already-sent")
        );
        assert!(store.claim_next(120).unwrap().is_none());
        assert!(store.inflight().unwrap().is_empty());
    }

    #[test]
    fn canonical_sent_state_wins_over_legacy_pending() {
        let dir = TestDirectory::new();
        dir.legacy(&[], &["mapped"]);
        let mut store = dir.open();
        let value = notification("canonical", 101);
        store
            .ingest("webhook", "guid", "push", "body", Some(&value), 90)
            .unwrap();
        assert_eq!(store.claim_next(110).unwrap().unwrap().key, "canonical");
        store.mark_sent("canonical", "canonical-sent", 111).unwrap();
        ingest(&mut store, EVENTS_SOURCE, "mapped", &value);
        assert_eq!(store.status().unwrap()["sent"], 1);
        assert_eq!(store.status().unwrap()["pending"], 0);
        assert_eq!(
            row(&mut store, "canonical").discord_message_id.as_deref(),
            Some("canonical-sent")
        );
        assert!(store.claim_next(120).unwrap().is_none());
    }

    #[test]
    fn historical_legacy_pending_is_suppressed_when_mapped() {
        let dir = TestDirectory::new();
        dir.legacy(&[], &["mapped"]);
        let mut store = dir.open();
        let value = notification("historical", 99);
        assert_eq!(
            ingest(&mut store, EVENTS_SOURCE, "mapped", &value),
            IngestOutcome::Ignored
        );
        assert_eq!(store.status().unwrap()["notifications"], 1);
        assert!(store.claim_next(120).unwrap().is_none());
    }

    #[test]
    fn legacy_import_is_one_time_and_attaches_fingerprints_after_mapping() {
        let dir = TestDirectory::new();
        dir.legacy(&["seen"], &["pending"]);
        {
            let mut store = dir.open();
            assert_eq!(store.status().unwrap()["next_poll_after"], 150);
            assert_eq!(store.status().unwrap()["receipts"], 2);
            // A temporarily ignored legacy record must still map later.
            store
                .ingest(EVENTS_SOURCE, "seen", "push", "body", None, 110)
                .unwrap();
            let value = notification("canonical-seen", 101);
            assert_eq!(
                store
                    .ingest(EVENTS_SOURCE, "seen", "push", "body", Some(&value), 111)
                    .unwrap(),
                IngestOutcome::Ignored
            );
        }
        fs::write(dir.0.join("state.json"), b"corrupted obsolete backup").unwrap();
        let mut store = dir.open();
        assert_eq!(store.status().unwrap()["receipts"], 2);
        assert_eq!(store.status().unwrap()["suppressed"], 1);
        assert_eq!(
            store.claim_next(120).unwrap().unwrap().key,
            "legacy:pending"
        );
    }

    #[test]
    fn failed_legacy_import_can_be_retried_without_losing_checkpoint() {
        let dir = TestDirectory::new();
        fs::write(dir.0.join("state.json"), b"{invalid").unwrap();
        assert!(Store::open(&dir.0, "channel", 100).is_err());
        dir.legacy(&["seen"], &["pending"]);
        let mut store = dir.open();
        assert_eq!(store.status().unwrap()["receipts"], 2);
        assert_eq!(store.status().unwrap()["pending"], 1);
        assert!(
            fs::read(dir.0.join("state.json"))
                .unwrap()
                .starts_with(b"{")
        );
    }
}
