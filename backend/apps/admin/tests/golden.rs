//! Serialization goldens (binding-spec §3): the EmitUnpopulated shape of the
//! response codec, pinned byte-exact. The authoritative cross-backend pin is
//! the differential harness; these lock the Rust side against drift.
//!
//! Shape rules in play (protojson + EmitUnpopulated, as implemented by
//! prost-reflect's `skip_default_fields(false)`):
//! * presence-tracked fields (proto3 `optional`, singular messages, oneof
//!   members) that are unset stay OMITTED — even under EmitUnpopulated;
//! * plain (non-optional) scalars emit their default; 64-bit integers emit as
//!   strings; unset repeated fields emit as [].
//!
//! §3.1 adds static redaction to the same tail: the plan built over this
//! pool mutates the response before the encoder runs; the redact goldens
//! pin the masked wire shapes against the contract's own annotations.

use admin::proto::dict::service::v1::{Language, ListLanguageResponse};
use admin::proto::identity::service::v1::{ListUserResponse, User};

#[test]
fn zero_value_presence_only_message_omits_everything() {
    // Language's every field is `optional` — unset presence fields are
    // omitted, so the zero value serializes to the empty object.
    let bytes = rushwind_http_binding::codec::serialize_response(
        admin::pool(),
        "dict.service.v1.Language",
        &Language::default(),
        None,
    )
    .unwrap();
    assert_eq!(String::from_utf8_lossy(&bytes), "{}");
}

#[test]
fn zero_value_plain_fields_emit_defaults() {
    // ListLanguageResponse: `repeated Language items` and plain `uint64
    // total` — unset repeated emits [], the unset 64-bit scalar emits its
    // default AS A STRING per protojson.
    let bytes = rushwind_http_binding::codec::serialize_response(
        admin::pool(),
        "dict.service.v1.ListLanguageResponse",
        &ListLanguageResponse::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&bytes),
        "{\"items\":[],\"total\":\"0\"}"
    );
}

/// A ListUserResponse with the sensitive fields populated, run through
/// the real plan (the corpus anchors: identity/user.proto's email rule
/// `keep_local_first: 2`, mobile rule `keep_first: 3 keep_last: 4`, and
/// the `element = { nested: true }` envelope on `items`).
fn populated_user_list() -> ListUserResponse {
    ListUserResponse {
        items: vec![User {
            username: Some("zhangsan".into()),
            email: Some("zhangsan@example.com".into()),
            mobile: Some("13812345678".into()),
            ..Default::default()
        }],
        total: 1,
    }
}

#[test]
fn redact_plan_masks_the_contract_sensitive_fields() {
    let bytes = rushwind_http_binding::codec::serialize_response(
        admin::pool(),
        "identity.service.v1.ListUserResponse",
        &populated_user_list(),
        Some((
            admin::redact_plan(),
            "/identity.service.v1.UserService/List",
        )),
    )
    .unwrap();
    // Byte-exact: the masked values sit in field-number order among the
    // EmitUnpopulated defaults (repeated empties, omitted absent
    // optionals) — the exact wire shape the reference deployment emits.
    assert_eq!(
        String::from_utf8_lossy(&bytes),
        "{\"items\":[{\"orgUnitIds\":[],\"orgUnitNames\":[],\"positionIds\":[],\
         \"positionNames\":[],\"roleIds\":[],\"roles\":[],\"roleNames\":[],\
         \"username\":\"zhangsan\",\"email\":\"zh******@example.com\",\
         \"mobile\":\"138****5678\"}],\"total\":\"1\"}"
    );
}

#[test]
fn method_skip_leaves_the_response_untouched() {
    // The i_user BFF marks its write operations `(redact.method_skip)`; a
    // skipped operation's response serializes without redaction even when
    // the message type carries rules.
    let bytes = rushwind_http_binding::codec::serialize_response(
        admin::pool(),
        "identity.service.v1.ListUserResponse",
        &populated_user_list(),
        Some((admin::redact_plan(), "/admin.service.v1.UserService/Update")),
    )
    .unwrap();
    assert!(
        String::from_utf8_lossy(&bytes).contains("\"mobile\":\"13812345678\""),
        "skipped operation must not mask: {}",
        String::from_utf8_lossy(&bytes)
    );
}

#[test]
fn no_plan_disables_redaction() {
    let bytes = rushwind_http_binding::codec::serialize_response(
        admin::pool(),
        "identity.service.v1.ListUserResponse",
        &populated_user_list(),
        None,
    )
    .unwrap();
    assert!(
        String::from_utf8_lossy(&bytes).contains("\"mobile\":\"13812345678\""),
        "no plan must not mask: {}",
        String::from_utf8_lossy(&bytes)
    );
}
