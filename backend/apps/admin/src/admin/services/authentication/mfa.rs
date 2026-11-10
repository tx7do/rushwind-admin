//! The MFA login-challenge store: a short-lived Redis row that holds
//! the authenticated payload between the password grant and the
//! TOTP verification.

use crate::admin::token::{UserTokenPayload, CLIENT_TYPE_ADMIN};

use super::AuthenticationService;

/// The MFA login-challenge window.
const MFA_CHALLENGE_TTL: u64 = 300;

impl AuthenticationService {
    /// The MFA login challenge row (`mfa:login:{opId}`, 5 min).
    pub(super) async fn set_mfa_challenge(
        &self,
        op_id: &str,
        payload: &UserTokenPayload,
    ) -> Result<(), String> {
        let mut conn = self.state.redis.clone();
        let body = serde_json::json!({ "payload": payload, "clientType": CLIENT_TYPE_ADMIN });
        let _: Result<(), _> = redis::AsyncCommands::set_ex(
            &mut conn,
            format!("mfa:login:{op_id}"),
            body.to_string(),
            MFA_CHALLENGE_TTL,
        )
        .await;
        Ok(())
    }

    /// Wired with the MFA login branch (storage phase).
    #[allow(dead_code)]
    pub(super) async fn take_mfa_challenge(&self, op_id: &str) -> Option<UserTokenPayload> {
        let mut conn = self.state.redis.clone();
        let raw: Option<String> =
            redis::AsyncCommands::get(&mut conn, format!("mfa:login:{op_id}"))
                .await
                .ok()?;
        let value: serde_json::Value = serde_json::from_str(raw.as_deref()?).ok()?;
        serde_json::from_value(value.get("payload").cloned()?).ok()
    }
}
