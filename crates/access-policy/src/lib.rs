// SPDX-License-Identifier: AGPL-3.0-only
//! Pure authorization rules, NOT token verification or a complete access system.
//! Inputs must come from trusted server/agent state AFTER cryptographic validation.
//! Network payloads must never be mapped directly into this context.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    View,
    Input,
    FileTransfer,
    Clipboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessMode {
    Attended,
    Unattended,
}

#[derive(Debug)]
pub struct Context<'a> {
    pub operator_organization: &'a str,
    pub device_organization: &'a str,
    pub operator_id: &'a str,
    pub operator_enabled: bool,
    pub device_enabled: bool,
    pub operator_allowed: bool,
    pub grant_operator: &'a str,
    pub grant_device: &'a str,
    pub device_id: &'a str,
    pub grant_not_before: u64,
    pub grant_expires_at: u64,
    pub now: u64,
    pub grant_revoked: bool,
    pub mode: AccessMode,
    pub unattended_enabled: bool,
    /// Consent must be scoped to this session and its requested capabilities.
    pub session_consent: bool,
    pub policy_capabilities: &'a [Capability],
    pub grant_capabilities: &'a [Capability],
    pub local_capabilities: &'a [Capability],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denial {
    InvalidIdentity,
    OrganizationMismatch,
    Disabled,
    OperatorNotAllowed,
    WrongGrantBinding,
    InvalidGrantTime,
    Revoked,
    ConsentRequired,
    UnattendedDisabled,
    EmptyRequest,
    CapabilityDenied,
}

/// All requested capabilities must be allowed by server policy, grant AND agent.
/// Same function must gate both direct and relayed paths in future integration.
pub fn authorize(ctx: &Context<'_>, requested: &[Capability]) -> Result<(), Denial> {
    if [ctx.operator_id, ctx.device_id, ctx.operator_organization, ctx.device_organization]
        .iter().any(|v| v.trim().is_empty()) {
        return Err(Denial::InvalidIdentity);
    }
    if ctx.operator_organization != ctx.device_organization {
        return Err(Denial::OrganizationMismatch);
    }
    if !ctx.operator_enabled || !ctx.device_enabled { return Err(Denial::Disabled); }
    if !ctx.operator_allowed { return Err(Denial::OperatorNotAllowed); }
    if ctx.grant_operator != ctx.operator_id || ctx.grant_device != ctx.device_id {
        return Err(Denial::WrongGrantBinding);
    }
    if ctx.grant_expires_at <= ctx.grant_not_before
        || ctx.now < ctx.grant_not_before || ctx.now >= ctx.grant_expires_at {
        return Err(Denial::InvalidGrantTime);
    }
    if ctx.grant_revoked { return Err(Denial::Revoked); }
    match ctx.mode {
        AccessMode::Attended if !ctx.session_consent => return Err(Denial::ConsentRequired),
        AccessMode::Unattended if !ctx.unattended_enabled => return Err(Denial::UnattendedDisabled),
        _ => {}
    }
    if requested.is_empty() { return Err(Denial::EmptyRequest); }
    for capability in requested {
        if !ctx.policy_capabilities.contains(capability)
            || !ctx.grant_capabilities.contains(capability)
            || !ctx.local_capabilities.contains(capability) {
            return Err(Denial::CapabilityDenied);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const ALL: &[Capability] = &[Capability::View, Capability::Input, Capability::FileTransfer, Capability::Clipboard];
    fn context() -> Context<'static> {
        Context {
            operator_organization: "org-a", device_organization: "org-a",
            operator_id: "operator-a", device_id: "device-a",
            operator_enabled: true, device_enabled: true, operator_allowed: true,
            grant_operator: "operator-a", grant_device: "device-a",
            grant_not_before: 100, grant_expires_at: 200, now: 150,
            grant_revoked: false, mode: AccessMode::Attended,
            unattended_enabled: false, session_consent: true,
            policy_capabilities: ALL, grant_capabilities: ALL, local_capabilities: ALL,
        }
    }
    #[test] fn allows_valid_session() { assert_eq!(authorize(&context(), ALL), Ok(())); }
    #[test] fn rejects_other_organization() {
        let mut c = context(); c.device_organization = "org-b";
        assert_eq!(authorize(&c, ALL), Err(Denial::OrganizationMismatch));
    }
    #[test] fn rejects_empty_identity() {
        let mut c = context(); c.operator_id = " ";
        assert_eq!(authorize(&c, ALL), Err(Denial::InvalidIdentity));
    }
    #[test] fn rejects_other_device_grant() {
        let mut c = context(); c.grant_device = "device-b";
        assert_eq!(authorize(&c, ALL), Err(Denial::WrongGrantBinding));
    }
    #[test] fn rejects_other_operator_grant() {
        let mut c = context(); c.grant_operator = "operator-b";
        assert_eq!(authorize(&c, ALL), Err(Denial::WrongGrantBinding));
    }
    #[test] fn time_boundaries_are_explicit() {
        for (time, allowed) in [(99, false), (100, true), (199, true), (200, false)] {
            let mut c = context(); c.now = time;
            assert_eq!(authorize(&c, ALL).is_ok(), allowed);
        }
    }
    #[test] fn rejects_bad_time_interval() {
        let mut c = context(); c.grant_expires_at = c.grant_not_before;
        assert_eq!(authorize(&c, ALL), Err(Denial::InvalidGrantTime));
    }
    #[test] fn revocation_is_checked_on_each_call() {
        let mut c = context(); assert_eq!(authorize(&c, ALL), Ok(())); c.grant_revoked = true;
        assert_eq!(authorize(&c, ALL), Err(Denial::Revoked));
    }
    #[test] fn attended_requires_consent() {
        let mut c = context(); c.session_consent = false;
        assert_eq!(authorize(&c, ALL), Err(Denial::ConsentRequired));
    }
    #[test] fn unattended_requires_opt_in() {
        let mut c = context(); c.mode = AccessMode::Unattended; c.session_consent = false;
        assert_eq!(authorize(&c, ALL), Err(Denial::UnattendedDisabled));
        c.unattended_enabled = true; assert_eq!(authorize(&c, ALL), Ok(()));
    }
    #[test] fn view_does_not_imply_input() {
        let mut c = context(); c.grant_capabilities = &[Capability::View];
        assert_eq!(authorize(&c, &[Capability::View]), Ok(()));
        assert_eq!(authorize(&c, &[Capability::Input]), Err(Denial::CapabilityDenied));
    }
    #[test] fn local_policy_cannot_be_overridden() {
        let mut c = context(); c.local_capabilities = &[Capability::View];
        assert_eq!(authorize(&c, ALL), Err(Denial::CapabilityDenied));
    }
    #[test] fn server_policy_cannot_be_overridden() {
        let mut c = context(); c.policy_capabilities = &[];
        assert_eq!(authorize(&c, ALL), Err(Denial::CapabilityDenied));
    }
    #[test] fn disabled_device_is_denied() {
        let mut c = context(); c.device_enabled = false;
        assert_eq!(authorize(&c, ALL), Err(Denial::Disabled));
    }
    #[test] fn disabled_operator_is_denied() {
        let mut c = context(); c.operator_enabled = false;
        assert_eq!(authorize(&c, ALL), Err(Denial::Disabled));
    }
    #[test] fn missing_acl_is_denied() {
        let mut c = context(); c.operator_allowed = false;
        assert_eq!(authorize(&c, ALL), Err(Denial::OperatorNotAllowed));
    }
    #[test] fn empty_request_is_denied() {
        assert_eq!(authorize(&context(), &[]), Err(Denial::EmptyRequest));
    }
}
