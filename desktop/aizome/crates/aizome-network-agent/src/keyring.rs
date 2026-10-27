use crate::{
    AgentError, Result,
    process::{self, Cancel},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::process::Command;

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub action: String,
    pub uuid: String,
    #[serde(default)]
    pub setting: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub unlock: bool,
}

pub trait Store: Send + Sync {
    fn lookup(&self, uuid: &str, setting: &str, unlock: bool, cancel: &Cancel) -> Result<HashMap<String, String>>;
    /// Each saved credential's setting and key, read without its secret
    /// and without unlocking.
    fn saved(&self, uuid: &str, cancel: &Cancel) -> Result<Vec<(String, String)>>;
    fn store(&self, uuid: &str, setting: &str, key: &str, value: &str, label: &str, cancel: &Cancel) -> Result<()>;
    fn forget(&self, uuid: &str, setting: &str, key: &str, cancel: &Cancel) -> Result<()>;
    fn delete(&self, uuid: &str, cancel: &Cancel) -> Result<()>;
}

pub struct Keyring;
impl Keyring {
    fn call(&self, r: Request, cancel: &Cancel) -> Result<Vec<(String, String)>> {
        let exe = std::env::current_exe().map_err(|_| AgentError::Failed("Cannot locate keyring worker".into()))?;
        let bytes = process::run(
            Command::new(exe).arg("--keyring"),
            serde_json::to_vec(&r).map_err(|_| AgentError::Failed("Invalid keyring request".into()))?,
            cancel,
            AgentError::Failed("Cannot access the login keyring".into()),
        )?;
        serde_json::from_slice(&bytes).map_err(|_| AgentError::Failed("Cannot access the login keyring".into()))
    }
    fn request(action: &str, uuid: &str, setting: &str, key: &str) -> Request {
        Request {
            action: action.into(),
            uuid: uuid.into(),
            setting: setting.into(),
            key: key.into(),
            value: String::new(),
            label: String::new(),
            unlock: false,
        }
    }
}
impl Store for Keyring {
    fn lookup(&self, uuid: &str, setting: &str, unlock: bool, c: &Cancel) -> Result<HashMap<String, String>> {
        let r = Request { unlock, ..Self::request("lookup", uuid, setting, "") };
        self.call(r, c).map(|values| values.into_iter().collect())
    }
    fn saved(&self, uuid: &str, c: &Cancel) -> Result<Vec<(String, String)>> {
        self.call(Self::request("list", uuid, "", ""), c)
    }
    fn store(&self, uuid: &str, setting: &str, key: &str, value: &str, label: &str, c: &Cancel) -> Result<()> {
        let r = Request {
            value: value.into(),
            label: label.into(),
            unlock: true,
            ..Self::request("store", uuid, setting, key)
        };
        self.call(r, c).map(drop)
    }
    fn forget(&self, uuid: &str, setting: &str, key: &str, c: &Cancel) -> Result<()> {
        self.call(Self::request("delete", uuid, setting, key), c).map(drop)
    }
    fn delete(&self, uuid: &str, c: &Cancel) -> Result<()> {
        self.call(Self::request("delete", uuid, "", ""), c).map(drop)
    }
}

unsafe extern "C" {
    fn aizome_keyring(
        action: *const c_char,
        uuid: *const c_char,
        setting: *const c_char,
        key: *const c_char,
        value: *const c_char,
        label: *const c_char,
        unlock: i32,
        emit: extern "C" fn(*mut c_void, *const c_char, *const c_char),
        context: *mut c_void,
    ) -> i32;
}
extern "C" fn collect(context: *mut c_void, first: *const c_char, second: *const c_char) {
    // Pointers are owned by libsecret and valid for the duration of this callback.
    unsafe {
        let pairs = &mut *context.cast::<Vec<(String, String)>>();
        pairs.push((CStr::from_ptr(first).to_string_lossy().into(), CStr::from_ptr(second).to_string_lossy().into()));
    }
}
pub fn worker(r: Request) -> Result<Vec<(String, String)>> {
    if !matches!(r.action.as_str(), "lookup" | "list" | "store" | "delete") || r.uuid.is_empty() {
        return Err(AgentError::InvalidConnection("Invalid keyring operation".into()));
    }
    let args: Vec<_> = [&r.action, &r.uuid, &r.setting, &r.key, &r.value, &r.label]
        .into_iter()
        .map(|s| CString::new(s.as_str()))
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| AgentError::InvalidConnection("Invalid credential encoding".into()))?;
    let mut values = Vec::<(String, String)>::new();
    let result = unsafe {
        aizome_keyring(
            args[0].as_ptr(),
            args[1].as_ptr(),
            args[2].as_ptr(),
            args[3].as_ptr(),
            args[4].as_ptr(),
            args[5].as_ptr(),
            r.unlock.into(),
            collect,
            (&mut values as *mut Vec<(String, String)>).cast(),
        )
    };
    if result != 0 {
        return Err(AgentError::Failed("Cannot access the login keyring".into()));
    }
    Ok(values)
}
