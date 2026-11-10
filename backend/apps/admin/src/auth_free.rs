/// The auth-free operations: the deployment's whitelist registrations
/// (commented-out entries excluded), pinned 1:1 as
/// (service full name, proto method name) pairs. Consumed by the build
/// script's generator config (the public/gated mount split) and re-exported
/// here for the corpus guard test and the differential harness.
pub const AUTH_FREE: &[(&str, &str)] = &[
    ("admin.service.v1.AuthenticationService", "Login"),
    ("admin.service.v1.AuthenticationService", "GenerateCaptcha"),
    ("admin.service.v1.AuthenticationService", "VerifyCaptcha"),
    ("admin.service.v1.AuthenticationService", "RefreshToken"),
    ("admin.service.v1.MfaService", "VerifyMFAChallenge"),
    ("admin.service.v1.AccessKeyService", "IssueToken"),
    ("admin.service.v1.AuthenticationService", "ForgotPassword"),
    (
        "admin.service.v1.AuthenticationService",
        "ResetPasswordByCode",
    ),
];
