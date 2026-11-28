//! Docs server — serves the API documentation surface over the embedded
//! OpenAPI document (a contract-build artifact, embedded like the
//! reference's `go:embed` of the same file — the runtime *config*
//! yamls live on disk):
//!
//! * `/q/openapi.yaml` — the raw protoc-gen-openapi spec (YAML);
//! * `/q/swagger-ui` — Swagger UI reading that spec;
//! * `/q/redoc` — Redoc reading the same spec.
//!
//! UI assets load from their CDNs; the switches ride the admin route
//! pack's settings node (`enable_swagger` / `enable_redoc`).

use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

use crate::admin::assets::OPENAPI_DATA;

/// The docs switches wire (the admin route pack's settings node): one
/// mount flag per docs surface. A missing node or field leaves its
/// switch off.
#[derive(Debug, Default, Deserialize)]
pub struct Wire {
    /// Whether the Swagger UI mounts.
    #[serde(default)]
    pub enable_swagger: bool,
    /// Whether Redoc mounts.
    #[serde(default)]
    pub enable_redoc: bool,
}

/// Parses the docs switches from the pack's settings node. A missing
/// node leaves both switches off.
pub fn wire(settings: serde_json::Value) -> Result<Wire, rushwind_bootstrap::BootstrapError> {
    crate::admin::server::settings_or_default(settings, "docs switches")
}

/// The raw OpenAPI document (YAML).
pub async fn openapi_yaml() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/yaml; charset=utf-8")],
        OPENAPI_DATA.to_string(),
    )
}

/// Swagger UI shell: the widget loads the spec from `/q/openapi.yaml`.
pub async fn swagger_ui() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        SWAGGER_UI_HTML.to_string(),
    )
}

/// Redoc shell: the widget loads the spec from `/q/openapi.yaml`.
pub async fn redoc() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        REDOC_HTML.to_string(),
    )
}

/// Assembles the docs routes. The spec mounts when either UI is
/// enabled; each UI mounts under its own switch.
pub fn router(enable_swagger: bool, enable_redoc: bool) -> Router {
    let mut router = Router::new();
    if enable_swagger || enable_redoc {
        router = router.route("/q/openapi.yaml", get(openapi_yaml));
    }
    if enable_swagger {
        router = router.route("/q/swagger-ui", get(swagger_ui));
    }
    if enable_redoc {
        router = router.route("/q/redoc", get(redoc));
    }
    router
}

const SWAGGER_UI_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8"/>
  <title>Admin API — Swagger UI</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css"/>
</head>
<body>
<div id="swagger-ui"></div>
<script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
<script>
  SwaggerUIBundle({
    url: "/q/openapi.yaml",
    dom_id: "#swagger-ui",
    deepLinking: true,
    withCredentials: true,
  });
</script>
</body>
</html>"##;

const REDOC_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8"/>
  <title>Admin API — Redoc</title>
  <style>body { margin: 0; padding: 0; }</style>
</head>
<body>
<redoc spec-url="/q/openapi.yaml"></redoc>
<script src="https://cdn.redoc.ly/redoc/latest/bundles/redoc.standalone.js"></script>
</body>
</html>"#;
