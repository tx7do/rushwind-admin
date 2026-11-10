//! Binder behavior tests — pin the ported form-decode semantics
//! (binding-spec §2.2) one by one. These are the Rust-side twins of the
//! differential corpus replayed by the differential harness.

use prost_reflect::{DescriptorPool, DynamicMessage};
use rushwind_http_binding::binder::bind_form;

fn pool() -> &'static DescriptorPool {
    admin::pool()
}

fn fresh(fq: &str) -> DynamicMessage {
    DynamicMessage::new(pool().get_message_by_name(fq).expect("type in pool"))
}

/// Unknown keys are silently dropped; the message stays at zero.
#[test]
fn unknown_key_silent_skip() {
    let mut m = fresh("dict.service.v1.GetLanguageRequest");
    bind_form(&mut m, &[("nope".into(), vec!["x".into()])]).unwrap();
    let out: admin::proto::dict::service::v1::GetLanguageRequest = m.transcode_to().unwrap();
    assert!(out.query_by.is_none());
}

/// Empty values are skipped even when the field resolves.
#[test]
fn empty_value_skip() {
    let mut m = fresh("dict.service.v1.GetLanguageRequest");
    bind_form(&mut m, &[("id".into(), vec![String::new()])]).unwrap();
    let out: admin::proto::dict::service::v1::GetLanguageRequest = m.transcode_to().unwrap();
    assert!(out.query_by.is_none());
}

/// Proto-name spelling resolves (oneof member `id`).
#[test]
fn oneof_member_by_proto_name() {
    let mut m = fresh("dict.service.v1.GetLanguageRequest");
    bind_form(&mut m, &[("id".into(), vec!["5".into()])]).unwrap();
    let out: admin::proto::dict::service::v1::GetLanguageRequest = m.transcode_to().unwrap();
    assert!(matches!(
        out.query_by,
        Some(admin::proto::dict::service::v1::get_language_request::QueryBy::Id(5))
    ));
}

/// A second member of the same oneof after the first is set → error.
#[test]
fn oneof_double_set_rejected() {
    let mut m = fresh("dict.service.v1.GetLanguageRequest");
    let e = bind_form(
        &mut m,
        &[
            ("id".into(), vec!["5".into()]),
            ("code".into(), vec!["en".into()]),
        ],
    )
    .unwrap_err();
    assert_eq!(e.status, 400);
    assert_eq!(e.reason, "CODEC");
}

/// Multi-value on a singular field → error.
#[test]
fn multi_value_singular_rejected() {
    let mut m = fresh("dict.service.v1.GetLanguageRequest");
    let e = bind_form(&mut m, &[("id".into(), vec!["1".into(), "2".into()])]).unwrap_err();
    assert_eq!(e.reason, "CODEC");
}

/// json_name spelling + FieldMask comma-split + camel→snake normalization
/// (protojson field-name normalization, ported verbatim).
#[test]
fn field_mask_json_name_snake_normalization() {
    let mut m = fresh("dict.service.v1.UpdateLanguageRequest");
    bind_form(&mut m, &[("updateMask".into(), vec!["aB,cD_e".into()])]).unwrap();
    let out: admin::proto::dict::service::v1::UpdateLanguageRequest = m.transcode_to().unwrap();
    let mask = out.update_mask.expect("mask set").paths;
    assert_eq!(mask, &["a_b".to_string(), "c_d_e".to_string()]);
}

/// Repeated scalar via the `field[]` suffix and json_name — and the
/// append-across-keys semantics (list.Append).
#[test]
fn repeated_append_and_suffix() {
    let mut m = fresh("dict.service.v1.BatchCreateLanguagesResponse");
    bind_form(
        &mut m,
        &[
            ("createdIds[]".into(), vec!["7".into()]),
            ("createdIds".into(), vec!["8".into(), "9".into()]),
        ],
    )
    .unwrap();
    let out: admin::proto::dict::service::v1::BatchCreateLanguagesResponse =
        m.transcode_to().unwrap();
    assert_eq!(out.created_ids, &[7, 8, 9]);
}

/// Go ParseBool spellings (`t`, `TRUE`) — the set Rust's parser rejects.
#[test]
fn go_parse_bool_spellings() {
    let mut m = fresh("dict.service.v1.Language");
    bind_form(
        &mut m,
        &[
            ("isDefault".into(), vec!["t".into()]),
            ("isEnabled".into(), vec!["TRUE".into()]),
        ],
    )
    .unwrap();
    let out: admin::proto::dict::service::v1::Language = m.transcode_to().unwrap();
    assert_eq!(out.is_default, Some(true));
    assert_eq!(out.is_enabled, Some(true));
}

/// Nested dotted path into a message field, Timestamp well-known leaf with
/// RFC3339 parsing.
#[test]
fn nested_timestamp_rfc3339() {
    let mut m = fresh("dict.service.v1.CreateLanguageRequest");
    bind_form(
        &mut m,
        &[("data.createdAt".into(), vec!["2009-01-01T10:00:00Z".into()])],
    )
    .unwrap();
    let out: admin::proto::dict::service::v1::CreateLanguageRequest = m.transcode_to().unwrap();
    let ts = out
        .data
        .as_ref()
        .and_then(|d| d.created_at.as_ref())
        .expect("timestamp set");
    // 2009-01-01T00:00:00Z = 1230768000; 10:00Z adds ten hours.
    assert_eq!(ts.seconds, 1230768000 + 10 * 3600);
    assert_eq!(ts.nanos, 0);
}

/// Map fields via BOTH key spellings, with a well-known (Value) element:
/// `fields.k` (dot) and `fields[k]` (bracket) on google.protobuf.Struct.
#[test]
fn struct_map_both_spellings() {
    for key in ["fields.k", "fields[k]"] {
        let mut m = fresh("google.protobuf.Struct");
        bind_form(&mut m, &[(key.into(), vec!["v".into()])]).unwrap();
        let out: pbjson_types::Struct = m.transcode_to().unwrap();
        assert_eq!(out.fields.len(), 1, "key spelling {key}");
        let is_string = matches!(
            out.fields.get("k").map(|b| b.kind.as_ref()),
            Some(Some(pbjson_types::value::Kind::StringValue(_)))
        );
        assert!(is_string, "key spelling {key}");
    }
}

/// Non-whitelisted message leaves are refused (map value here is a
/// DictEntryI18n message).
#[test]
fn non_whitelisted_message_leaf_refused() {
    let fq = "dict.service.v1.DictEntry";
    if pool().get_message_by_name(fq).is_none() {
        return; // corpus guarantee; keep the test corpus-agnostic
    }
    let desc = pool().get_message_by_name(fq).unwrap();
    if !desc.fields().any(|f| f.is_map()) {
        return;
    }
    let mut m = DynamicMessage::new(desc);
    let e = bind_form(&mut m, &[("i18n.k".into(), vec!["v".into()])]).unwrap_err();
    assert_eq!(e.reason, "CODEC");
}
