use ayx_one_api::{
    LEGACY_OTP_COMPATIBILITY_VERSION, LegacyOtpAdapter, LegacyOtpCompatibilityContract,
};

/// Characterization gate: this test is intentionally independent of a live
/// tenant and fails if a future adapter silently changes the protected
/// endpoint/cookie/retry contract.
#[test]
fn legacy_otp_compatibility_contract_is_locked() {
    let contract = LegacyOtpCompatibilityContract::default();
    contract.validate().expect("legacy OTP contract drifted");
    assert_eq!(contract.version, LEGACY_OTP_COMPATIBILITY_VERSION);
    assert_eq!(LegacyOtpAdapter::contract(), contract);
}

/// Live checks are opt-in because a real login consumes an OTP email and can
/// mint a PAT. CI and normal local test runs must never send one implicitly.
#[test]
fn legacy_otp_live_canary_is_explicitly_gated() {
    if std::env::var("AYX_AUTH_LIVE_CANARY").ok().as_deref() != Some("1") {
        return;
    }
    // The canary harness supplies the real interactive/live invocation outside
    // this unit process. Keep the release gate visible and deterministic here.
    assert_eq!(
        std::env::var("AYX_AUTH_ROLLOUT").ok().as_deref(),
        Some("canary"),
        "live auth canary requires AYX_AUTH_ROLLOUT=canary"
    );
}
