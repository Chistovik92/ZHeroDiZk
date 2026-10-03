// SPDX-License-Identifier: AGPL-3.0-only
//! Session grants: a short-lived, signed statement "operator O may use capabilities C on device D
//! until time T". The control server signs it (Ed25519); the device agent verifies it, checks
//! that it has not been seen before, and only then asks the access policy.
//!
//! Token format: `zhg1.<kid>.<payload>.<signature>` where `kid` is the first 8 bytes of the
//! SHA-256 of the signer's public key in hex, `payload` is the claims JSON in unpadded
//! URL-safe base64 and `signature` signs the ASCII text `zherodizk-grant-v1.<kid>.<payload>`
//! exactly as transmitted (so no JSON canonicalisation is needed).

use std::collections::HashMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zherodizk_access_policy::{authorize, AccessMode, Capability, Context, Denial};

pub const TOKEN_PREFIX: &str = "zhg1";
pub const VERSION: u8 = 1;
/// Longest lifetime a device accepts, whatever the server signed.
pub const MAX_LIFETIME_SECS: u64 = 300;
/// How far into the future a grant's start time may be (clock differences).
pub const START_SKEW_SECS: u64 = 30;
pub const MAX_TOKEN_LEN: usize = 4096;
pub const CAPABILITY_NAMES: [&str; 4] = ["view", "input", "file_transfer", "clipboard"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claims {
    pub v: u8,
    /// Unique id of this grant (used for replay protection and revocation).
    pub jti: String,
    pub org: String,
    pub operator: String,
    pub device: String,
    pub caps: Vec<String>,
    /// `attended` (someone at the device consents) or `unattended`.
    pub mode: String,
    pub nbf: u64,
    pub exp: u64,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum GrantError {
    #[error("token is malformed")]
    Malformed,
    #[error("token is too long")]
    TooLong,
    #[error("signing key is not trusted")]
    UnknownKey,
    #[error("signature is invalid")]
    BadSignature,
    #[error("unsupported grant version")]
    Version,
    #[error("grant is not valid yet")]
    NotYetValid,
    #[error("grant has expired")]
    Expired,
    #[error("grant lifetime is not acceptable")]
    Lifetime,
    #[error("grant is for another device")]
    WrongDevice,
    #[error("grant content is not acceptable")]
    Content,
    #[error("grant has already been used")]
    Replay,
}

/// Identifier of a verification key inside tokens.
pub fn key_id(key: &VerifyingKey) -> String {
    let digest = Sha256::digest(key.to_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

fn signing_text(kid: &str, payload: &str) -> String {
    format!("zherodizk-grant-v1.{kid}.{payload}")
}

/// Sign claims. The caller is responsible for the claims being what policy allows.
pub fn sign(claims: &Claims, key: &SigningKey) -> Result<String, GrantError> {
    let json = serde_json::to_vec(claims).map_err(|_| GrantError::Content)?;
    let payload = URL_SAFE_NO_PAD.encode(json);
    let kid = key_id(&key.verifying_key());
    let signature = key.sign(signing_text(&kid, &payload).as_bytes());
    Ok(format!("{TOKEN_PREFIX}.{kid}.{payload}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes())))
}

/// Verify a token: format, trusted key, signature, then the claims against `now` and the
/// device it is presented to. Replay is checked separately with [`ReplayCache`].
pub fn verify(
    token: &str,
    trusted: &[VerifyingKey],
    now: u64,
    expected_device: &str,
) -> Result<Claims, GrantError> {
    if token.len() > MAX_TOKEN_LEN {
        return Err(GrantError::TooLong);
    }
    let parts: Vec<&str> = token.split('.').collect();
    let [prefix, kid, payload, signature] = parts.as_slice() else {
        return Err(GrantError::Malformed);
    };
    if *prefix != TOKEN_PREFIX {
        return Err(GrantError::Malformed);
    }
    let key = trusted.iter().find(|k| key_id(k) == *kid).ok_or(GrantError::UnknownKey)?;
    let signature_bytes: [u8; 64] = URL_SAFE_NO_PAD
        .decode(signature)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or(GrantError::Malformed)?;
    key.verify(signing_text(kid, payload).as_bytes(), &Signature::from_bytes(&signature_bytes))
        .map_err(|_| GrantError::BadSignature)?;

    let json = URL_SAFE_NO_PAD.decode(payload).map_err(|_| GrantError::Malformed)?;
    let claims: Claims = serde_json::from_slice(&json).map_err(|_| GrantError::Malformed)?;
    check_claims(&claims, now, expected_device)?;
    Ok(claims)
}

fn check_claims(claims: &Claims, now: u64, expected_device: &str) -> Result<(), GrantError> {
    if claims.v != VERSION {
        return Err(GrantError::Version);
    }
    if claims.exp <= claims.nbf || claims.exp - claims.nbf > MAX_LIFETIME_SECS {
        return Err(GrantError::Lifetime);
    }
    if claims.nbf > now.saturating_add(START_SKEW_SECS) {
        return Err(GrantError::NotYetValid);
    }
    if now >= claims.exp {
        return Err(GrantError::Expired);
    }
    if claims.device != expected_device {
        return Err(GrantError::WrongDevice);
    }
    let valid_mode = claims.mode == "attended" || claims.mode == "unattended";
    let valid_caps = !claims.caps.is_empty()
        && claims.caps.iter().all(|c| CAPABILITY_NAMES.contains(&c.as_str()))
        && [&claims.jti, &claims.org, &claims.operator, &claims.device].iter().all(|v| !v.trim().is_empty());
    if !valid_mode || !valid_caps {
        return Err(GrantError::Content);
    }
    Ok(())
}

/// Remembers grant ids until they expire, so one grant opens at most one session.
#[derive(Debug, Default)]
pub struct ReplayCache {
    seen: HashMap<String, u64>,
}

impl ReplayCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Ok` the first time a grant id is presented, `Err(Replay)` afterwards.
    pub fn accept(&mut self, claims: &Claims, now: u64) -> Result<(), GrantError> {
        self.seen.retain(|_, exp| *exp > now);
        if self.seen.contains_key(&claims.jti) {
            return Err(GrantError::Replay);
        }
        self.seen.insert(claims.jti.clone(), claims.exp);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

pub fn parse_capability(name: &str) -> Option<Capability> {
    match name {
        "view" => Some(Capability::View),
        "input" => Some(Capability::Input),
        "file_transfer" => Some(Capability::FileTransfer),
        "clipboard" => Some(Capability::Clipboard),
        _ => None,
    }
}

/// What the device itself knows and decides; none of it comes from the network.
#[derive(Debug)]
pub struct LocalState<'a> {
    pub org_id: &'a str,
    pub device_id: &'a str,
    pub device_enabled: bool,
    pub unattended_enabled: bool,
    /// Capabilities the local owner allows at all.
    pub local_capabilities: &'a [Capability],
    /// The grant id is on the device's revocation list.
    pub grant_revoked: bool,
    /// Someone at the device consented to this session (needed in attended mode).
    pub session_consent: bool,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConnectionDenied {
    #[error("grant is for another device")]
    WrongDevice,
    #[error("grant content is not acceptable")]
    Content,
    #[error("access policy denied the request: {0:?}")]
    Policy(Denial),
}

/// Final decision on a connection request with an already verified, not yet replayed grant.
/// `requested` are the capabilities the operator asks for in this session.
pub fn authorize_connection(
    claims: &Claims,
    local: &LocalState<'_>,
    requested: &[Capability],
    now: u64,
) -> Result<(), ConnectionDenied> {
    if claims.device != local.device_id {
        return Err(ConnectionDenied::WrongDevice);
    }
    let grant_caps: Vec<Capability> = claims
        .caps
        .iter()
        .map(|c| parse_capability(c))
        .collect::<Option<Vec<_>>>()
        .ok_or(ConnectionDenied::Content)?;
    let mode = match claims.mode.as_str() {
        "attended" => AccessMode::Attended,
        "unattended" => AccessMode::Unattended,
        _ => return Err(ConnectionDenied::Content),
    };
    let context = Context {
        operator_organization: &claims.org,
        device_organization: local.org_id,
        operator_id: &claims.operator,
        operator_enabled: true,
        device_enabled: local.device_enabled,
        // The control server already applied the operator's access rules when it signed the grant.
        operator_allowed: true,
        grant_operator: &claims.operator,
        grant_device: &claims.device,
        device_id: local.device_id,
        grant_not_before: claims.nbf,
        grant_expires_at: claims.exp,
        now,
        grant_revoked: local.grant_revoked,
        mode,
        unattended_enabled: local.unattended_enabled,
        session_consent: local.session_consent,
        policy_capabilities: &grant_caps,
        grant_capabilities: &grant_caps,
        local_capabilities: local.local_capabilities,
    };
    authorize(&context, requested).map_err(ConnectionDenied::Policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::OsRng, RngCore};

    const NOW: u64 = 1_800_000_000;

    fn key() -> SigningKey {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        SigningKey::from_bytes(&seed)
    }

    fn claims() -> Claims {
        Claims {
            v: 1,
            jti: "grant-1".into(),
            org: "org-a".into(),
            operator: "operator-a".into(),
            device: "device-a".into(),
            caps: vec!["view".into(), "input".into()],
            mode: "attended".into(),
            nbf: NOW,
            exp: NOW + 60,
        }
    }

    fn issue(claims: &Claims, key: &SigningKey) -> String {
        sign(claims, key).unwrap()
    }

    #[test]
    fn round_trip() {
        let k = key();
        let token = issue(&claims(), &k);
        assert!(token.starts_with("zhg1."));
        assert_eq!(verify(&token, &[k.verifying_key()], NOW + 1, "device-a").unwrap(), claims());
    }

    #[test]
    fn rejects_other_keys_tampering_and_garbage() {
        let k = key();
        let other = key();
        let token = issue(&claims(), &k);
        let trusted = [k.verifying_key()];
        assert_eq!(verify(&token, &[other.verifying_key()], NOW, "device-a"), Err(GrantError::UnknownKey));

        // Changing the payload breaks the signature.
        let parts: Vec<&str> = token.split('.').collect();
        let mut altered = claims();
        altered.caps.push("clipboard".into());
        let forged_payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&altered).unwrap());
        let forged = format!("{}.{}.{}.{}", parts[0], parts[1], forged_payload, parts[3]);
        assert_eq!(verify(&forged, &trusted, NOW, "device-a"), Err(GrantError::BadSignature));
        // Re-labelling the key id fails too: the signature covers it.
        let relabelled = format!("{}.{}.{}.{}", parts[0], key_id(&other.verifying_key()), parts[2], parts[3]);
        assert_eq!(
            verify(&relabelled, &[k.verifying_key(), other.verifying_key()], NOW, "device-a"),
            Err(GrantError::BadSignature)
        );

        for bad in ["", "zhg1", "zhg1.a.b", "zhg2.a.b.c", "a.b.c.d.e", "zhg1..."] {
            assert!(verify(bad, &trusted, NOW, "device-a").is_err(), "{bad:?}");
        }
        assert_eq!(verify(&"x".repeat(MAX_TOKEN_LEN + 1), &trusted, NOW, "device-a"), Err(GrantError::TooLong));
    }

    #[test]
    fn time_window_is_enforced() {
        let k = key();
        let trusted = [k.verifying_key()];
        let token = issue(&claims(), &k);
        assert!(verify(&token, &trusted, NOW, "device-a").is_ok());
        assert!(verify(&token, &trusted, NOW + 59, "device-a").is_ok());
        assert_eq!(verify(&token, &trusted, NOW + 60, "device-a"), Err(GrantError::Expired));
        // A little clock difference at the start is tolerated, a lot is not.
        assert!(verify(&token, &trusted, NOW - START_SKEW_SECS, "device-a").is_ok());
        assert_eq!(verify(&token, &trusted, NOW - START_SKEW_SECS - 1, "device-a"), Err(GrantError::NotYetValid));
    }

    #[test]
    fn long_or_inverted_lifetimes_are_refused_even_when_signed() {
        let k = key();
        let trusted = [k.verifying_key()];
        let mut long = claims();
        long.exp = long.nbf + MAX_LIFETIME_SECS + 1;
        assert_eq!(verify(&issue(&long, &k), &trusted, NOW, "device-a"), Err(GrantError::Lifetime));
        let mut inverted = claims();
        inverted.exp = inverted.nbf;
        assert_eq!(verify(&issue(&inverted, &k), &trusted, NOW, "device-a"), Err(GrantError::Lifetime));
        let mut edge = claims();
        edge.exp = edge.nbf + MAX_LIFETIME_SECS;
        assert!(verify(&issue(&edge, &k), &trusted, NOW, "device-a").is_ok());
    }

    #[test]
    fn grant_is_bound_to_one_device() {
        let k = key();
        let token = issue(&claims(), &k);
        assert_eq!(verify(&token, &[k.verifying_key()], NOW, "device-b"), Err(GrantError::WrongDevice));
    }

    #[test]
    fn unacceptable_content_is_refused() {
        let k = key();
        let trusted = [k.verifying_key()];
        let mut cases = Vec::new();
        let mut c = claims();
        c.caps = vec![];
        cases.push(c);
        let mut c = claims();
        c.caps = vec!["root".into()];
        cases.push(c);
        let mut c = claims();
        c.mode = "whatever".into();
        cases.push(c);
        let mut c = claims();
        c.operator = "  ".into();
        cases.push(c);
        for case in cases {
            assert_eq!(verify(&issue(&case, &k), &trusted, NOW, "device-a"), Err(GrantError::Content), "{case:?}");
        }
        let mut v2 = claims();
        v2.v = 2;
        assert_eq!(verify(&issue(&v2, &k), &trusted, NOW, "device-a"), Err(GrantError::Version));
    }

    #[test]
    fn unknown_fields_are_refused() {
        let k = key();
        let payload = URL_SAFE_NO_PAD.encode(br#"{"v":1,"jti":"a","org":"o","operator":"u","device":"d","caps":["view"],"mode":"attended","nbf":1,"exp":2,"admin":true}"#);
        let kid = key_id(&k.verifying_key());
        let signature = k.sign(signing_text(&kid, &payload).as_bytes());
        let token = format!("zhg1.{kid}.{payload}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()));
        assert_eq!(verify(&token, &[k.verifying_key()], 1, "d"), Err(GrantError::Malformed));
    }

    #[test]
    fn key_rotation_accepts_either_trusted_key() {
        let old = key();
        let new = key();
        let trusted = [old.verifying_key(), new.verifying_key()];
        assert!(verify(&issue(&claims(), &old), &trusted, NOW, "device-a").is_ok());
        assert!(verify(&issue(&claims(), &new), &trusted, NOW, "device-a").is_ok());
        assert_ne!(key_id(&old.verifying_key()), key_id(&new.verifying_key()));
    }

    #[test]
    fn replay_cache_accepts_each_grant_once_and_forgets_expired_ones() {
        let mut cache = ReplayCache::new();
        let c = claims();
        assert_eq!(cache.accept(&c, NOW), Ok(()));
        assert_eq!(cache.accept(&c, NOW + 1), Err(GrantError::Replay));
        let mut other = claims();
        other.jti = "grant-2".into();
        assert_eq!(cache.accept(&other, NOW + 1), Ok(()));
        assert_eq!(cache.len(), 2);
        // After expiry the entries are dropped (and the grant itself no longer verifies).
        let mut later = claims();
        later.jti = "grant-3".into();
        later.nbf = NOW + 1000;
        later.exp = NOW + 1060;
        assert_eq!(cache.accept(&later, NOW + 1000), Ok(()));
        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());
    }

    fn local(caps: &[Capability]) -> LocalState<'_> {
        LocalState {
            org_id: "org-a",
            device_id: "device-a",
            device_enabled: true,
            unattended_enabled: false,
            local_capabilities: caps,
            grant_revoked: false,
            session_consent: true,
        }
    }

    const ALL: [Capability; 4] =
        [Capability::View, Capability::Input, Capability::FileTransfer, Capability::Clipboard];

    #[test]
    fn connection_is_allowed_only_inside_grant_and_local_limits() {
        let c = claims();
        assert_eq!(authorize_connection(&c, &local(&ALL), &[Capability::View], NOW + 1), Ok(()));
        assert_eq!(
            authorize_connection(&c, &local(&ALL), &[Capability::View, Capability::Input], NOW + 1),
            Ok(())
        );
        // Not in the grant.
        assert_eq!(
            authorize_connection(&c, &local(&ALL), &[Capability::Clipboard], NOW + 1),
            Err(ConnectionDenied::Policy(Denial::CapabilityDenied))
        );
        // Granted, but the device owner does not allow it locally.
        assert_eq!(
            authorize_connection(&c, &local(&[Capability::View]), &[Capability::Input], NOW + 1),
            Err(ConnectionDenied::Policy(Denial::CapabilityDenied))
        );
    }

    #[test]
    fn local_state_can_always_say_no() {
        let c = claims();
        let mut state = local(&ALL);
        state.grant_revoked = true;
        assert_eq!(authorize_connection(&c, &state, &[Capability::View], NOW + 1), Err(ConnectionDenied::Policy(Denial::Revoked)));
        let mut state = local(&ALL);
        state.device_enabled = false;
        assert_eq!(authorize_connection(&c, &state, &[Capability::View], NOW + 1), Err(ConnectionDenied::Policy(Denial::Disabled)));
        let mut state = local(&ALL);
        state.session_consent = false;
        assert_eq!(authorize_connection(&c, &state, &[Capability::View], NOW + 1), Err(ConnectionDenied::Policy(Denial::ConsentRequired)));
        let mut state = local(&ALL);
        state.org_id = "org-b";
        assert_eq!(authorize_connection(&c, &state, &[Capability::View], NOW + 1), Err(ConnectionDenied::Policy(Denial::OrganizationMismatch)));
        let mut state = local(&ALL);
        state.device_id = "device-b";
        assert_eq!(authorize_connection(&c, &state, &[Capability::View], NOW + 1), Err(ConnectionDenied::WrongDevice));
        assert_eq!(authorize_connection(&c, &local(&ALL), &[], NOW + 1), Err(ConnectionDenied::Policy(Denial::EmptyRequest)));
        assert_eq!(authorize_connection(&c, &local(&ALL), &[Capability::View], NOW + 61), Err(ConnectionDenied::Policy(Denial::InvalidGrantTime)));
    }

    #[test]
    fn unattended_grants_need_local_opt_in_and_no_consent() {
        let mut c = claims();
        c.mode = "unattended".into();
        let mut state = local(&ALL);
        state.session_consent = false;
        assert_eq!(authorize_connection(&c, &state, &[Capability::View], NOW + 1), Err(ConnectionDenied::Policy(Denial::UnattendedDisabled)));
        state.unattended_enabled = true;
        assert_eq!(authorize_connection(&c, &state, &[Capability::View], NOW + 1), Ok(()));
    }

    #[test]
    fn capability_names_cover_the_policy_enum() {
        for name in CAPABILITY_NAMES {
            assert!(parse_capability(name).is_some(), "{name}");
        }
        assert!(parse_capability("root").is_none());
    }
}
