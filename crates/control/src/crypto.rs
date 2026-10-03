// SPDX-License-Identifier: AGPL-3.0-only
//! Encryption of secrets at rest (AES-256-GCM, random 96-bit nonce stored in front of the data).

use aes_gcm::{aead::Aead, aead::KeyInit, Aes256Gcm, Nonce};
use rand::{rngs::OsRng, RngCore};

pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum CryptoError {
    #[error("encryption failed")]
    Seal,
    #[error("data is malformed or the key is wrong")]
    Open,
}

#[allow(deprecated)]
pub fn seal(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::Seal)?;
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let encrypted = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|_| CryptoError::Seal)?;
    let mut out = Vec::with_capacity(NONCE_LEN + encrypted.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&encrypted);
    Ok(out)
}

#[allow(deprecated)]
pub fn open(key: &[u8; KEY_LEN], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if data.len() < NONCE_LEN + TAG_LEN {
        return Err(CryptoError::Open);
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::Open)?;
    let (nonce, encrypted) = data.split_at(NONCE_LEN);
    cipher
        .decrypt(Nonce::from_slice(nonce), encrypted)
        .map_err(|_| CryptoError::Open)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; KEY_LEN] = [7; KEY_LEN];

    #[test]
    fn round_trip_and_fresh_nonce() {
        let a = seal(&KEY, b"secret value").unwrap();
        let b = seal(&KEY, b"secret value").unwrap();
        assert_ne!(a, b, "nonces must differ");
        assert_eq!(open(&KEY, &a).unwrap(), b"secret value");
        assert!(!a.windows(12).any(|w| w == b"secret value"));
    }

    #[test]
    fn wrong_key_tampering_and_short_input_fail() {
        let sealed = seal(&KEY, b"secret value").unwrap();
        assert_eq!(open(&[8; KEY_LEN], &sealed), Err(CryptoError::Open));
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert_eq!(open(&KEY, &tampered), Err(CryptoError::Open));
        assert_eq!(open(&KEY, &sealed[..20]), Err(CryptoError::Open));
        assert_eq!(open(&KEY, &[]), Err(CryptoError::Open));
    }
}
