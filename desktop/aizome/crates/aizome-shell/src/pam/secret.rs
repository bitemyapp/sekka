use std::ffi::CStr;
use zeroize::Zeroizing;

/// Our copy of the password, retained until PAM finishes its conversation.
/// The input belongs to the caller; PAM owns and frees its response copies.
pub(super) struct Password(Zeroizing<Vec<u8>>);

impl Password {
    pub(super) fn new(password: &str) -> Result<Self, String> {
        if password.as_bytes().contains(&0) {
            return Err("The password can't hold a NUL".into());
        }
        let mut bytes = Zeroizing::new(Vec::with_capacity(password.len() + 1));
        bytes.extend_from_slice(password.as_bytes());
        bytes.push(0);
        Ok(Self(bytes))
    }

    pub(super) fn as_c_str(&self) -> &CStr {
        CStr::from_bytes_with_nul(&self.0).expect("Password maintains one trailing NUL")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pam_gets_the_complete_password_and_a_terminator() {
        for value in ["", "audit-fixture", "藍染"] {
            let password = Password::new(value).unwrap();
            assert_eq!(password.as_c_str().to_bytes(), value.as_bytes());
            assert_eq!(password.as_c_str().to_bytes_with_nul().last(), Some(&0));
            // Exercises zeroization through mutable owned storage under Miri.
            drop(password);
        }
    }

    #[test]
    fn embedded_nul_cannot_authenticate_a_truncated_password() {
        assert!(Password::new("right\0wrong").is_err());
    }
}
