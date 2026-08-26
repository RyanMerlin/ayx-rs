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
