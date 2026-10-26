//! Users against a fake AccountsService on a private bus.

mod common;

use aizome_sys::users::{AccountType, DeleteUser, Users};
use aizome_sys::{Error, Wake};
use common::{eventually, path};
use std::sync::{Arc, Mutex};
use zbus::fdo;
use zbus::object_server::{ObjectServer, SignalEmitter};
use zbus::zvariant::OwnedObjectPath;

#[derive(Default)]
struct Log {
    passwords: Vec<(String, String)>,
    created: Vec<(String, String, i32)>,
    deleted: Vec<(i64, bool)>,
}

struct Manager {
    log: Arc<Mutex<Log>>,
    users: Vec<OwnedObjectPath>,
}

#[zbus::interface(name = "org.freedesktop.Accounts")]
impl Manager {
    fn list_cached_users(&self) -> Vec<OwnedObjectPath> {
        self.users.clone()
    }

    async fn create_user(
        &mut self,
        name: String,
        real_name: String,
        account_type: i32,
        #[zbus(object_server)] server: &ObjectServer,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<OwnedObjectPath> {
        self.log.lock().unwrap().created.push((name.clone(), real_name.clone(), account_type));
        let user = path("/org/freedesktop/Accounts/User1002");
        server.at(user.as_str(), FakeUser::new(1002, &name, &real_name, account_type, self.log.clone())).await?;
        self.users.push(user.clone());
        Manager::user_added(&emitter, user.clone()).await?;
        Ok(user)
    }

    async fn delete_user(
        &mut self,
        uid: i64,
        remove_files: bool,
        #[zbus(object_server)] server: &ObjectServer,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.log.lock().unwrap().deleted.push((uid, remove_files));
        let user = path(&format!("/org/freedesktop/Accounts/User{uid}"));
        server.remove::<FakeUser, _>(user.as_str()).await?;
        self.users.retain(|u| *u != user);
        Manager::user_deleted(&emitter, user).await?;
        Ok(())
    }

    #[zbus(signal)]
    async fn user_added(emitter: &SignalEmitter<'_>, user: OwnedObjectPath) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn user_deleted(emitter: &SignalEmitter<'_>, user: OwnedObjectPath) -> zbus::Result<()>;
}

struct FakeUser {
    uid: u64,
    name: String,
    real_name: String,
    account_type: i32,
    log: Arc<Mutex<Log>>,
}

impl FakeUser {
    fn new(uid: u64, name: &str, real_name: &str, account_type: i32, log: Arc<Mutex<Log>>) -> FakeUser {
        FakeUser { uid, name: name.into(), real_name: real_name.into(), account_type, log }
    }
}

#[zbus::interface(name = "org.freedesktop.Accounts.User")]
impl FakeUser {
    #[zbus(property)]
    fn uid(&self) -> u64 {
        self.uid
    }
    #[zbus(property)]
    fn user_name(&self) -> String {
        self.name.clone()
    }
    #[zbus(property)]
    fn real_name(&self) -> String {
        self.real_name.clone()
    }
    #[zbus(property)]
    fn account_type(&self) -> i32 {
        self.account_type
    }
    #[zbus(property)]
    fn login_time(&self) -> i64 {
        1_792_056_771
    }
    #[zbus(property)]
    fn system_account(&self) -> bool {
        self.uid < 1000
    }
    async fn set_real_name(
        &mut self,
        name: String,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.real_name = name;
        FakeUser::changed(&emitter).await?;
        Ok(())
    }
    async fn set_account_type(
        &mut self,
        kind: i32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.account_type = kind;
        FakeUser::changed(&emitter).await?;
        Ok(())
    }
    fn set_password(&self, crypted: String, hint: String) {
        self.log.lock().unwrap().passwords.push((crypted, hint));
    }
    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// crypt(3)'s check of a SHA-512 hash.
fn verify(password: &str, hashed: &str) -> bool {
    let parts: Vec<&str> = hashed.split('$').collect();
    let [_, "6", rounds, salt, hash] = parts.as_slice() else { return false };
    let Some(rounds) = rounds.strip_prefix("rounds=").and_then(|r| r.parse().ok()) else { return false };
    let params = sha_crypt::Sha512Params::new(rounds).unwrap();
    sha_crypt::sha512_crypt_b64(password.as_bytes(), salt.as_bytes(), &params).is_ok_and(|h| h == *hash)
}

#[test]
fn accounts_names_passwords_creation_and_deletion() {
    let bus = test_bus!();
    let log = Arc::new(Mutex::new(Log::default()));
    let _fake = bus
        .builder()
        .name("org.freedesktop.Accounts")
        .unwrap()
        .serve_at(
            "/org/freedesktop/Accounts",
            Manager {
                log: log.clone(),
                users: vec![
                    path("/org/freedesktop/Accounts/User1000"),
                    path("/org/freedesktop/Accounts/User1001"),
                    path("/org/freedesktop/Accounts/User998"),
                ],
            },
        )
        .unwrap()
        .serve_at("/org/freedesktop/Accounts/User1000", FakeUser::new(1000, "callen", "Chris", 1, log.clone()))
        .unwrap()
        .serve_at("/org/freedesktop/Accounts/User1001", FakeUser::new(1001, "guest", "", 0, log.clone()))
        .unwrap()
        .serve_at("/org/freedesktop/Accounts/User998", FakeUser::new(998, "nixbld", "", 0, log.clone()))
        .unwrap()
        .build()
        .unwrap();
    let users = Users::with_bus(Some(bus.bus()), Wake::none());
    assert!(eventually(|| users.snapshot().users.len() == 2), "{:?}", users.snapshot());
    let state = users.snapshot();
    let names: Vec<&str> = state.users.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(names, ["callen", "guest"], "system accounts are left out");
    assert_eq!(state.administrators().count(), 1);
    assert!(state.user("callen").unwrap().last_login.is_some());
    let guest = state.user("guest").unwrap().clone();
    assert_eq!(guest.display_name(), "guest");

    users.set_real_name(&guest, "Guest Account").wait().unwrap();
    assert!(eventually(|| users.snapshot().user("guest").is_some_and(|u| u.real_name == "Guest Account")));
    users.set_account_type(&guest, AccountType::Administrator).wait().unwrap();
    assert!(eventually(|| users.snapshot().administrators().count() == 2));

    users.set_password(&guest, "hunter2!", "a hint").wait().unwrap();
    {
        let log = log.lock().unwrap();
        let (hashed, hint) = &log.passwords[0];
        assert!(verify("hunter2!", hashed), "{hashed}");
        assert!(!hashed.contains("hunter2"), "only the hash travels");
        assert_eq!(hint, "a hint");
    }
    assert!(matches!(users.set_password(&guest, "", "").wait(), Err(Error::InvalidArgument(_))));

    assert!(matches!(users.create_user("Bad Name", "", AccountType::Standard).wait(), Err(Error::InvalidArgument(_))));
    let created = users.create_user("ada", "Ada Lovelace", AccountType::Standard).wait().unwrap();
    assert_eq!(created, "/org/freedesktop/Accounts/User1002");
    assert!(eventually(|| users.snapshot().user("ada").is_some()));
    assert_eq!(log.lock().unwrap().created, [("ada".to_owned(), "Ada Lovelace".to_owned(), 0)]);

    let ada = users.snapshot().user("ada").unwrap().clone();
    users.delete_user(DeleteUser::confirm(&ada, true)).wait().unwrap();
    assert_eq!(log.lock().unwrap().deleted, [(1002, true)]);
    assert!(eventually(|| users.snapshot().user("ada").is_none()));
    // A confirmation for an account that is gone does nothing.
    assert!(matches!(users.delete_user(DeleteUser::confirm(&ada, false)).wait(), Err(Error::NotFound(_))));
}
