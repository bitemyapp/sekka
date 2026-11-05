// SPDX-License-Identifier: GPL-3.0-or-later
//! Whether a store that won't open was locked to a TPM that's gone, and
//! should be set aside for a new one, or might still open. "The unseal
//! failed while a TPM is there" also fits a busy TPM, a damaged token, or a
//! storage root key (SRK) that was only evicted, so the TPM's SRK decides:
//! the store records it when made, and a different one means the TPM was
//! cleared or replaced. A store set aside is retried at every boot and
//! merged back if it opens, so a wrong guess costs time, not networks.

/// What the TPM says its SRK is now (`systemd-analyze srk`).
#[derive(Clone, Debug, PartialEq)]
pub enum Srk {
    /// It didn't answer.
    Unreadable,
    /// It has none stored.
    Missing,
    Known(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decision {
    /// Leave the store: it may open next time.
    Keep,
    /// Store the SRK again (it derives from the TPM's seed, so a cleared
    /// TPM gives a different one) and decide again.
    RecreateSrk,
    /// Count this boot; set the store aside only after a few in a row.
    Count,
    /// Set the store aside and make a new one.
    Rotate,
}

/// Failing boots in a row, with the same SRK, before the store is set aside
/// anyway (its token or header may be damaged).
pub const SAME_SRK_BOOTS: u32 = 3;

/// `failures` counts earlier failing boots with the same SRK.
pub fn decide(now: &Srk, recorded: Option<&[u8]>, failures: u32) -> Decision {
    match (now, recorded) {
        (Srk::Unreadable, _) => Decision::Keep,
        (Srk::Missing, _) => Decision::RecreateSrk,
        (Srk::Known(now), Some(recorded)) if now.as_slice() != recorded => Decision::Rotate,
        // The same SRK, or none recorded (a power loss right after the store
        // was made): it may be the token, so only after a few boots.
        (Srk::Known(_), _) if failures + 1 >= SAME_SRK_BOOTS => Decision::Rotate,
        (Srk::Known(_), _) => Decision::Count,
    }
}

/// Rotated stores kept, newest first.
pub const KEPT: usize = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tpm_that_doesnt_answer_never_rotates() {
        assert_eq!(decide(&Srk::Unreadable, Some(b"a"), 99), Decision::Keep);
    }

    #[test]
    fn a_missing_srk_is_recreated_before_deciding() {
        assert_eq!(decide(&Srk::Missing, Some(b"a"), 0), Decision::RecreateSrk);
    }

    #[test]
    fn a_different_srk_means_a_cleared_or_replaced_tpm() {
        assert_eq!(
            decide(&Srk::Known(b"b".to_vec()), Some(b"a"), 0),
            Decision::Rotate
        );
    }

    #[test]
    fn the_same_srk_rotates_only_after_a_few_boots() {
        let same = Srk::Known(b"a".to_vec());
        assert_eq!(decide(&same, Some(b"a"), 0), Decision::Count);
        assert_eq!(decide(&same, Some(b"a"), 1), Decision::Count);
        assert_eq!(decide(&same, Some(b"a"), 2), Decision::Rotate);
        assert_eq!(decide(&same, None, 0), Decision::Count);
    }
}
