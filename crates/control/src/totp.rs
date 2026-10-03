// SPDX-License-Identifier: AGPL-3.0-only
//! TOTP (RFC 6238, HMAC-SHA1, 6 digits, 30-second steps) on top of HOTP (RFC 4226).

use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

pub const DIGITS: u32 = 6;
pub const PERIOD: u64 = 30;
pub const SECRET_LEN: usize = 20;

/// HOTP value for a counter with the given number of digits.
pub fn hotp(secret: &[u8], counter: u64, digits: u32) -> u32 {
    let mut mac = HmacSha1::new_from_slice(secret).expect("HMAC accepts keys of any length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let binary = ((u32::from(digest[offset]) & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    binary % 10u32.pow(digits)
}

pub fn step_for(unix_time: u64) -> u64 {
    unix_time / PERIOD
}

pub fn code_at(secret: &[u8], unix_time: u64) -> String {
    format!("{:0width$}", hotp(secret, step_for(unix_time), DIGITS), width = DIGITS as usize)
}

/// Constant-time comparison of two equal-length digit strings.
fn same_code(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Accepts the current step and `window` steps either side. Returns the matched step so the
/// caller can refuse to accept the same or an older step twice.
pub fn verify(secret: &[u8], code: &str, unix_time: u64, window: u64) -> Option<u64> {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let current = step_for(unix_time);
    let mut matched = None;
    // Check every step (no early exit) so timing does not reveal which one matched.
    for step in current.saturating_sub(window)..=current.saturating_add(window) {
        let expected = format!("{:0width$}", hotp(secret, step, DIGITS), width = DIGITS as usize);
        if same_code(expected.as_bytes(), code.as_bytes()) {
            matched = Some(step);
        }
    }
    matched
}

pub fn generate_secret() -> Vec<u8> {
    let mut secret = vec![0u8; SECRET_LEN];
    OsRng.fill_bytes(&mut secret);
    secret
}

pub fn secret_to_base32(secret: &[u8]) -> String {
    BASE32_NOPAD.encode(secret)
}

fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// URI for authenticator apps (the de-facto Key URI format).
pub fn otpauth_uri(issuer: &str, account: &str, secret: &[u8]) -> String {
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits={}&period={}",
        percent_encode(issuer),
        percent_encode(account),
        secret_to_base32(secret),
        percent_encode(issuer),
        DIGITS,
        PERIOD
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC_SECRET: &[u8] = b"12345678901234567890";

    #[test]
    fn rfc4226_hotp_vectors() {
        let expected = [755224, 287082, 359152, 969429, 338314, 254676, 287922, 162583, 399871, 520489];
        for (counter, want) in expected.iter().enumerate() {
            assert_eq!(hotp(RFC_SECRET, counter as u64, 6), *want);
        }
    }

    #[test]
    fn rfc6238_sha1_vectors() {
        // 8-digit values from RFC 6238 Appendix B; the 6-digit code is their last six digits.
        let vectors = [
            (59u64, 94287082u32),
            (1111111109, 7081804),
            (1111111111, 14050471),
            (1234567890, 89005924),
            (2000000000, 69279037),
            (20000000000, 65353130),
        ];
        for (time, eight) in vectors {
            assert_eq!(hotp(RFC_SECRET, step_for(time), 8), eight, "time {time}");
            assert_eq!(code_at(RFC_SECRET, time), format!("{:06}", eight % 1_000_000), "time {time}");
        }
    }

    #[test]
    fn window_accepts_neighbours_but_not_further() {
        let now = 1_700_000_000;
        let current = code_at(RFC_SECRET, now);
        assert_eq!(verify(RFC_SECRET, &current, now, 1), Some(step_for(now)));
        assert_eq!(verify(RFC_SECRET, &code_at(RFC_SECRET, now - PERIOD), now, 1), Some(step_for(now) - 1));
        assert_eq!(verify(RFC_SECRET, &code_at(RFC_SECRET, now + PERIOD), now, 1), Some(step_for(now) + 1));
        assert_eq!(verify(RFC_SECRET, &code_at(RFC_SECRET, now - 3 * PERIOD), now, 1), None);
    }

    #[test]
    fn malformed_codes_are_rejected() {
        let now = 1_700_000_000;
        for bad in ["", "12345", "1234567", "12345a", "١٢٣٤٥٦", "      "] {
            assert_eq!(verify(RFC_SECRET, bad, now, 1), None, "{bad:?}");
        }
        let good = code_at(RFC_SECRET, now);
        let spaced = format!("{} {}", &good[..3], &good[3..]);
        assert_eq!(verify(RFC_SECRET, &spaced, now, 1), Some(step_for(now)));
    }

    #[test]
    fn secret_generation_and_encoding() {
        let a = generate_secret();
        let b = generate_secret();
        assert_eq!(a.len(), SECRET_LEN);
        assert_ne!(a, b);
        let encoded = secret_to_base32(&a);
        assert_eq!(BASE32_NOPAD.decode(encoded.as_bytes()).unwrap(), a);
    }

    #[test]
    fn otpauth_uri_is_escaped_and_complete() {
        let uri = otpauth_uri("ZHeroDiZk", "user+tag@example.org", RFC_SECRET);
        assert!(uri.starts_with("otpauth://totp/ZHeroDiZk:user%2Btag%40example.org?secret="));
        assert!(uri.contains("&issuer=ZHeroDiZk&algorithm=SHA1&digits=6&period=30"));
        assert!(uri.contains("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"));
    }
}
