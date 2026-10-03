// SPDX-License-Identifier: AGPL-3.0-only
//! Password policy and Argon2id hashing (library defaults: Argon2id v1.3, m=19 MiB, t=2, p=1).

use std::sync::OnceLock;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

pub const MIN_LEN: usize = 12;
pub const MAX_LEN: usize = 256;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PasswordError {
    #[error("password must be at least 12 characters")]
    TooShort,
    #[error("password must be at most 256 characters")]
    TooLong,
    #[error("password must not contain NUL characters")]
    Nul,
}

/// Length is counted in characters; the upper bound keeps hashing cost bounded.
pub fn validate(password: &str) -> Result<(), PasswordError> {
    let length = password.chars().count();
    if length < MIN_LEN {
        Err(PasswordError::TooShort)
    } else if length > MAX_LEN {
        Err(PasswordError::TooLong)
    } else if password.contains('\0') {
        Err(PasswordError::Nul)
    } else {
        Ok(())
    }
}

pub fn hash(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default().hash_password(password.as_bytes(), &salt)?.to_string())
}

/// A malformed stored hash never verifies.
pub fn verify(stored_hash: &str, password: &str) -> bool {
    match PasswordHash::new(stored_hash) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// Verification against a fixed hash, used when the account does not exist so that
/// unknown and known accounts cost about the same time.
pub fn verify_dummy(password: &str) -> bool {
    static DUMMY: OnceLock<String> = OnceLock::new();
    let stored = DUMMY.get_or_init(|| hash("dummy password for timing equalisation").unwrap_or_default());
    verify(stored, password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_limits() {
        assert_eq!(validate("short"), Err(PasswordError::TooShort));
        assert_eq!(validate("12345678901"), Err(PasswordError::TooShort));
        assert_eq!(validate("123456789012"), Ok(()));
        assert_eq!(validate(&"a".repeat(MAX_LEN)), Ok(()));
        assert_eq!(validate(&"a".repeat(MAX_LEN + 1)), Err(PasswordError::TooLong));
        assert_eq!(validate("abcdefghijk\0l"), Err(PasswordError::Nul));
    }

    #[test]
    fn length_counts_characters_not_bytes() {
        assert_eq!(validate(&"п".repeat(12)), Ok(()));
        assert_eq!(validate(&"п".repeat(11)), Err(PasswordError::TooShort));
    }

    #[test]
    fn hashes_are_argon2id_and_salted() {
        let first = hash("correct horse battery").unwrap();
        let second = hash("correct horse battery").unwrap();
        assert!(first.starts_with("$argon2id$"));
        assert_ne!(first, second);
        assert!(verify(&first, "correct horse battery"));
        assert!(verify(&second, "correct horse battery"));
        assert!(!verify(&first, "correct horse batterY"));
    }

    #[test]
    fn malformed_hash_does_not_verify() {
        assert!(!verify("", "anything at all"));
        assert!(!verify("not a hash", "anything at all"));
    }

    #[test]
    fn dummy_verification_never_succeeds_for_real_passwords() {
        assert!(!verify_dummy("some other password"));
    }
}
