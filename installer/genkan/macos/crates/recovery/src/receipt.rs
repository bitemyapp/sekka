// SPDX-License-Identifier: GPL-3.0-or-later
//! What this app has done for its installation, kept on the stub's System
//! volume beside its data (`Sekka/State/receipt.json`) so that a restart,
//! a crash or a power cut in the middle is noticed next time.
//!
//! Each step is written as started before it runs and as completed (or
//! failed) after, replaced atomically and synchronized. A step found
//! "started" is uncertain: the app reads the Mac's actual policy before
//! repeating it, and only ever repeats it with the same, checked, boot
//! object.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sekka_mac_contract::{RECOVERY_RECEIPT_FILE, RECOVERY_STATE_DIR, Uuid, canonical};

pub use sekka_mac_contract::{RecoveryReceipt as Receipt, RecoveryStep as Step};

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn at(volume: &Path) -> Self {
        Self {
            path: volume.join(RECOVERY_STATE_DIR).join(RECOVERY_RECEIPT_FILE),
        }
    }

    pub fn load(&self, installation: Uuid) -> Result<Receipt, String> {
        match fs::read(&self.path) {
            Ok(bytes) => {
                let r = Receipt::parse(&bytes).map_err(|e| format!("The record of this step is damaged: {e}"))?;
                if r.installation != installation {
                    return Err("The record of this step belongs to another installation.".into());
                }
                Ok(r)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Receipt::new(installation)),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn save(&self, receipt: &Receipt) -> Result<(), String> {
        let dir = self.path.parent().expect("a parent");
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = dir.join(".receipt.json.tmp");
        let bytes = canonical::to_vec(receipt).map_err(|e| e.to_string())?;
        let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(&bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        fs::File::open(dir).and_then(|d| d.sync_all()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipts_survive_and_belong_to_one_installation() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::at(dir.path());
        let id = Uuid::from_bytes([1; 16]);
        let mut r = store.load(id).unwrap();
        assert_eq!(r.enrollment, Step::NotStarted);
        r.enrollment = Step::Started { at: sekka_mac_contract::Timestamp::from_unix(1) };
        store.save(&r).unwrap();
        assert_eq!(store.load(id).unwrap(), r);
        assert!(store.load(Uuid::from_bytes([2; 16])).is_err());
    }
}
