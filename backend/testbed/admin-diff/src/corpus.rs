//! The case corpus: an auto-generated sweep plus the curated file cases.
//!
//! The sweep walks the generated route table (the same 203 registrations the
//! Go backend makes) and emits, per route, a matched-method probe against
//! the concrete path (path variables substituted with `1`) classified as:
//!
//! * gated routes — [`Kind::EnvelopeExact`]: both sides must answer the
//!   identical 401 `UNAUTHORIZED` / `missing bearer token` envelope bytes;
//! * public routes — [`Kind::Pending`]: the Rust side is a null stub until
//!   its module lands; both responses are recorded, nothing asserted;
//! * gen-product routes — class `sweep-gen`, [`Kind::Pending`]: routes
//!   whose service is a `.rush/` spec entity (rush gen entity's output)
//!   have no Go reference at all — the Go stack answers 404 where the
//!   Rust gate answers its 401, so an EnvelopeExact probe would fail on
//!   presence, not behavior. They ride Pending (record-only) until the
//!   upstream Go stack adopts the same surface, at which point the case
//!   graduates by hand into the gated class with real payload probes.
//!
//! GET routes additionally emit a HEAD probe (class `head-on-get`) — the
//! recorded micro-divergence where axum's `MethodFilter::GET` serves HEAD
//! while gorilla mux rejects it; the class is in the seed exemption set and
//! the probe merely quantifies it.
//!
//! The curated file (testbed/corpus/curated.json) carries the hand-built
//! cases: gate rejections with malformed bearer credentials, codec failures
//! on public body routes, and CORS preflights.

use std::collections::HashMap;

use serde::Deserialize;

use crate::compare::Kind;

/// One replay case.
#[derive(Debug, Clone)]
pub struct Case {
    /// Stable identifier (`sweep-<idx>`, `sweep-head-<idx>`, or the curated id).
    pub id: String,
    /// The exemption class (`sweep-gated`, `sweep-public`, `head-on-get`, or
    /// the curated class).
    pub class: String,
    /// The comparison contract.
    pub kind: Kind,
    /// HTTP method, uppercase.
    pub method: String,
    /// The concrete path (path variables substituted).
    pub path: String,
    /// Extra request headers.
    pub headers: Vec<(String, String)>,
    /// The request body, if any.
    pub body: Option<String>,
}

/// The file face of a curated case; mirrors [`Case`] minus the derived id.
#[derive(Debug, Deserialize)]
struct CuratedCase {
    id: String,
    class: String,
    kind: KindWire,
    method: String,
    path: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: Option<String>,
}

/// The wire spelling of the case kind.
#[derive(Debug, Deserialize)]
enum KindWire {
    #[serde(rename = "Routing")]
    Routing,
    #[serde(rename = "EnvelopeExact")]
    EnvelopeExact,
    #[serde(rename = "EnvelopeShape")]
    EnvelopeShape,
    #[serde(rename = "Cors")]
    Cors,
    #[serde(rename = "Pending")]
    Pending,
}

impl From<&KindWire> for Kind {
    fn from(w: &KindWire) -> Self {
        match w {
            KindWire::Routing => Kind::Routing,
            KindWire::EnvelopeExact => Kind::EnvelopeExact,
            KindWire::EnvelopeShape => Kind::EnvelopeShape,
            KindWire::Cors => Kind::Cors,
            KindWire::Pending => Kind::Pending,
        }
    }
}

/// Substitutes every `{var}` segment in a route template with `1`.
fn concretize(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        match rest[open..].find('}') {
            Some(close) => {
                out.push('1');
                rest = &rest[open + close + 1..];
            }
            None => {
                rest = &rest[open..];
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The classification of one sweep route (pure, unit-testable).
/// `gen_services` carries the fully-qualified service names of the
/// `.rush/` spec entities — rush gen entity's output surface.
pub fn classify_route(
    service_fq: &str,
    method_name: &str,
    shadowed: bool,
    auth_free: &[(&'static str, &'static str)],
    gen_services: &std::collections::BTreeSet<String>,
) -> (&'static str, Kind) {
    let gated = !auth_free
        .iter()
        .any(|(s, m)| *s == service_fq && *m == method_name);
    if gen_services.contains(service_fq) {
        // The Go reference does not serve this surface: presence itself
        // diverges, so the honest contract is record-only.
        return ("sweep-gen", Kind::Pending);
    }
    if shadowed {
        // The shadow set: the go stack's first-match mux routes
        // these paths to the earlier pattern route, whose path-variable
        // bind is malformed for the literal segment — the pre/post-auth
        // ordering divergence the exemption set registers.
        return ("router-shadow", Kind::EnvelopeShape);
    }
    if gated {
        ("sweep-gated", Kind::EnvelopeExact)
    } else {
        ("sweep-public", Kind::Pending)
    }
}

/// The `.rush/` spec directory's service fqs on the admin face —
/// `admin.service.v1.<Pascal>Service` per spec entity. The rig has no
/// rush-gen dependency, so the Pascal rule repeats here on purpose
/// (spec names are snake_case; the generator capitalizes per segment).
pub fn gen_service_fqs(rush_dir: &std::path::Path) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(rush_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e != "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(name) = value.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let pascal = name
            .split('_')
            .map(|part| {
                let mut cs = part.chars();
                match cs.next() {
                    Some(first) => first.to_ascii_uppercase().to_string() + cs.as_str(),
                    None => String::new(),
                }
            })
            .collect::<String>();
        out.insert(format!("admin.service.v1.{pascal}Service"));
    }
    out
}

/// Builds the sweep from the generated route table.
pub fn sweep(gen_services: &std::collections::BTreeSet<String>) -> Vec<Case> {
    let mut cases = Vec::new();
    for (idx, spec) in admin::proto::gen::routes::ROUTES.iter().enumerate() {
        let concrete = concretize(spec.path);
        let (class, kind) = classify_route(
            spec.service_fq,
            spec.method_name,
            spec.shadowed,
            admin::AUTH_FREE,
            gen_services,
        );
        cases.push(Case {
            id: format!("sweep-{idx}"),
            class: class.into(),
            kind,
            method: spec.method.to_ascii_uppercase(),
            path: concrete.clone(),
            headers: Vec::new(),
            body: None,
        });
        if spec.method.eq_ignore_ascii_case("GET") {
            cases.push(Case {
                id: format!("sweep-head-{idx}"),
                class: "head-on-get".into(),
                kind: Kind::Routing,
                method: "HEAD".into(),
                path: concrete,
                headers: Vec::new(),
                body: None,
            });
        }
    }
    cases
}

/// Loads the curated cases from `<dir>/curated.json`.
pub fn load_curated(dir: &str) -> Vec<Case> {
    let path = std::path::Path::new(dir).join("curated.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("curated corpus missing: {} (skipping)", path.display());
        return Vec::new();
    };
    match serde_json::from_str::<Vec<CuratedCase>>(&text) {
        Ok(list) => list
            .into_iter()
            .map(|c| Case {
                id: c.id,
                class: c.class,
                kind: Kind::from(&c.kind),
                method: c.method.to_ascii_uppercase(),
                path: c.path,
                headers: c.headers.into_iter().collect(),
                body: c.body,
            })
            .collect(),
        Err(e) => {
            eprintln!("curated corpus parse error: {e} (skipping)");
            Vec::new()
        }
    }
}

/// Loads the exemption map (class → justification) from the given file.
pub fn load_exemptions(path: &str) -> HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("exemption file missing: {path} (none applied)");
        return HashMap::new();
    };
    #[derive(Deserialize)]
    struct File {
        exemptions: Vec<Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        class: String,
        note: String,
    }
    match serde_json::from_str::<File>(&text) {
        Ok(f) => f
            .exemptions
            .into_iter()
            .map(|e| (e.class, e.note))
            .collect(),
        Err(e) => {
            eprintln!("exemption file parse error: {e} (none applied)");
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn gen(fq: &str) -> BTreeSet<String> {
        let mut set = BTreeSet::new();
        set.insert(fq.to_string());
        set
    }

    const AUTH_FREE: &[(&str, &str)] = &[("admin.service.v1.OpenService", "List")];

    #[test]
    fn gen_product_routes_ride_pending_before_everything() {
        // A gen-entity service: Pending even though it is gated and not
        // shadowed — the Go reference has no such surface.
        let (class, kind) = classify_route(
            "admin.service.v1.WidgetService",
            "List",
            false,
            AUTH_FREE,
            &gen("admin.service.v1.WidgetService"),
        );
        assert_eq!(class, "sweep-gen");
        assert_eq!(kind, Kind::Pending);

        // Shadowed gen routes stay gen (presence, not the shadow quirk).
        let (class, kind) = classify_route(
            "admin.service.v1.WidgetService",
            "List",
            true,
            AUTH_FREE,
            &gen("admin.service.v1.WidgetService"),
        );
        assert_eq!(class, "sweep-gen");
        assert_eq!(kind, Kind::Pending);
    }

    #[test]
    fn non_gen_routes_keep_their_classes() {
        let none = BTreeSet::new();
        let (class, kind) = classify_route(
            "admin.service.v1.UserService",
            "Get",
            false,
            AUTH_FREE,
            &none,
        );
        assert_eq!((class, kind), ("sweep-gated", Kind::EnvelopeExact));
        let (class, kind) = classify_route(
            "admin.service.v1.OpenService",
            "List",
            false,
            AUTH_FREE,
            &none,
        );
        assert_eq!((class, kind), ("sweep-public", Kind::Pending));
        let (class, kind) = classify_route(
            "admin.service.v1.UserService",
            "Get",
            true,
            AUTH_FREE,
            &none,
        );
        assert_eq!((class, kind), ("router-shadow", Kind::EnvelopeShape));
    }

    #[test]
    fn fqs_read_from_the_rush_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("widget.json"),
            r#"{"schema":1,"name":"widget","table":"sys_widgets","package":"widget.service.v1","route_prefix":"/admin/v1/widgets","fields":[],"code_field":null,"global":false,"group":null,"stack":null}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not a spec").unwrap();
        let fqs = gen_service_fqs(dir.path());
        assert_eq!(
            fqs,
            gen("admin.service.v1.WidgetService"),
            "txt ignored, name → admin-face fq"
        );
        // Empty dir → empty set.
        assert!(gen_service_fqs(dir.path().join("nope").as_path()).is_empty());
    }
}
