//! Golden bytes for the error envelope (binding-spec §4): the four fields
//! in field-number order, `code` as the bare numeric annotation value (not
//! the status-line text — a regression pinned here after it was caught in
//! the assembly smoke test), serde_json string escaping for reason/message,
//! `metadata` always `{}`, and the HTTP status mirroring the code.

use axum::http::StatusCode;
use rushwind_http_binding::envelope::StatusError;

#[tokio::test]
async fn envelope_exact_bytes() {
    let cases: &[(StatusError, u16, &str)] = &[
        (
            StatusError::new(401, "UNAUTHORIZED", "missing bearer token"),
            401,
            r#"{"code":401,"reason":"UNAUTHORIZED","message":"missing bearer token","metadata":{}}"#,
        ),
        (
            StatusError::new(500, "", "not implemented"),
            500,
            r#"{"code":500,"reason":"","message":"not implemented","metadata":{}}"#,
        ),
        (
            StatusError::new(400, "CODEC", "boom \"quoted\" \\ end"),
            400,
            r#"{"code":400,"reason":"CODEC","message":"boom \"quoted\" \\ end","metadata":{}}"#,
        ),
    ];
    for (err, want_status, want_body) in cases {
        let resp = rushwind_http_binding::envelope::error_response(err.clone());
        assert_eq!(resp.status(), StatusCode::from_u16(*want_status).unwrap());
        let content_type = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert_eq!(content_type, "application/json");
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("envelope body collectable");
        assert_eq!(
            std::str::from_utf8(&body).unwrap_or_default(),
            *want_body,
            "envelope bytes diverged for status {want_status}"
        );
    }
}
