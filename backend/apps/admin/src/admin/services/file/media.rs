//! The media rules face: the upload MIME whitelist, the content
//! sniffing and bucket routing the upload enforces, the signed public
//! media URL, and the signature image proxy. The transfer service
//! (`super::transfer`) applies them; the proxy is a public route whose
//! credential is the HMAC itself.

/// oss.MaxUploadSize (pkg/oss/module).
pub(super) const MAX_UPLOAD_SIZE: usize = 50 * 1024 * 1024;

/// The MIME whitelist (pkg/oss/module:19-44): prefixes plus exact
/// doc types.
pub(super) fn mime_allowed(mime: &str) -> bool {
    for prefix in ["image/", "video/", "audio/"] {
        if mime.starts_with(prefix) {
            return true;
        }
    }
    for exact in [
        "application/pdf",
        "application/msword",
        "application/zip",
        "application/x-zip-compressed",
        "application/vnd.ms-powerpoint",
        "application/vnd.ms-excel",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "text/plain",
    ] {
        if mime == exact {
            return true;
        }
    }
    false
}

pub(super) fn bucket_for_mime(mime: &str) -> &'static str {
    if mime.starts_with("image/") {
        "images"
    } else if mime.starts_with("video/") {
        "videos"
    } else if mime.starts_with("audio/") {
        "audios"
    } else if mime.starts_with("text/")
        || mime.contains("pdf")
        || mime.contains("word")
        || mime.contains("officedocument")
        || mime.contains("powerpoint")
        || mime.contains("excel")
        || mime.contains("msword")
    {
        "docs"
    } else {
        "files"
    }
}

/// Content sniffing over the leading magic bytes — the upload path
/// trusts the bytes, not the client-declared mime, so the bucket route
/// cannot be bypassed by a relabelled payload. `None` defers to the
/// client declaration.
pub(super) fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(b"%PDF") {
        return Some("application/pdf");
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return Some("application/zip");
    }
    if bytes.len() > 12 && &bytes[4..8] == b"ftyp" {
        return Some("video/mp4");
    }
    if bytes.starts_with(b"ID3") || (bytes.len() > 1 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0)
    {
        return Some("audio/mpeg");
    }
    None
}

/// The signed public media URL (`/admin/v1/file/image`): HMAC-SHA256
/// over `path|expires` under the crypto key; unset key yields an empty
/// URL (the reference behavior — rich-text embedding degrades, the
/// authorized download API still works).
pub(super) fn signed_media_url(crypto_key: Option<[u8; 32]>, storage_path: &str) -> String {
    let Some(key) = crypto_key else {
        return String::new();
    };
    let expires = chrono::Utc::now().timestamp() + 365 * 24 * 3600;
    let data = format!("{storage_path}|{expires}");
    use hmac::Mac as _;
    let mut mac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(&key).expect("hmac accepts any key length");
    mac.update(data.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    format!("/admin/v1/file/image?path={storage_path}&expires={expires}&sig={sig}")
}

/// The signature image proxy — a public route whose credential is the
/// HMAC: verify expiry then signature, stream the object from the
/// bucket the path names.
pub async fn image_proxy(
    axum::extract::State(state): axum::extract::State<
        std::sync::Arc<crate::admin::state::AppState>,
    >,
    params: axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let bad = |code: u16, text: &'static str| async move {
        (
            axum::http::StatusCode::from_u16(code).unwrap_or(axum::http::StatusCode::BAD_REQUEST),
            text,
        )
            .into_response()
    };
    let path = params.get("path").cloned().unwrap_or_default();
    let expires = params.get("expires").cloned().unwrap_or_default();
    let sig = params.get("sig").cloned().unwrap_or_default();
    if path.is_empty() || expires.is_empty() || sig.is_empty() {
        return bad(400, "missing parameters").await;
    }
    let Ok(expires_at) = expires.parse::<i64>() else {
        return bad(403, "url expired").await;
    };
    if chrono::Utc::now().timestamp() > expires_at {
        return bad(403, "url expired").await;
    }
    let Some(key) = crate::admin::crypto::crypto_key() else {
        return bad(403, "invalid signature").await;
    };
    use hmac::Mac as _;
    let mut mac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(&key).expect("hmac accepts any key length");
    mac.update(format!("{path}|{expires}").as_bytes());
    if hex::encode(mac.finalize().into_bytes()) != sig {
        return bad(403, "invalid signature").await;
    }

    // "/bucket/object" — the bucket routes to its engine.
    let media = path.trim_start_matches('/');
    let Some((bucket, object)) = media.split_once('/') else {
        return bad(400, "invalid path").await;
    };
    let Some(oss) = state.oss.as_ref().and_then(|o| o.storage(bucket)) else {
        return bad(404, "not found").await;
    };
    match oss.get(object).await {
        Ok(bytes) => {
            let ext = object.rsplit('.').next().unwrap_or("");
            let mime = match ext {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                _ => "application/octet-stream",
            };
            ([(axum::http::header::CONTENT_TYPE, mime)], bytes).into_response()
        }
        Err(_) => bad(404, "not found").await,
    }
}
