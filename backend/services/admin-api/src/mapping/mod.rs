//! The DB status literals ↔ the wire enum numbers, one table per proto
//! enum. Tables carry only the explicitly enumerated pairs; domain
//! fallbacks for unknown values stay at the call sites (`.unwrap_or`
//! with the same default the old hand-written matches had), so a
//! mismatched fallback pair can never be frozen into the table.
//!
//! `enum_map!` one-way arms emit `str -> Option<i32>`; both-way arms
//! additionally emit the inverse `i32 -> Option<&'static str>` —
//! duplicate numbers in a both-way table fail the match-arm check at
//! compile time.
//!
//! The submodule layout is the wire axis: a submodule is named after
//! the proto package its enums ride in (a table shared by two packages
//! takes the neutral name). A newly ported domain's enums land in the
//! submodule of its proto package — no judgment call. Everything is
//! re-exported flat, so call sites keep reading
//! `crate::mapping::<table>_of`.

/// One table per proto enum: the DB status literal ↔ the wire number.
///
/// * `$name { .. }` — emits `fn $name(&str) -> Option<i32>`;
/// * `$name + $rev { .. }` — additionally emits `fn $rev(i32) ->
///   Option<&'static str>` from the same pairs.
macro_rules! enum_map {
    ($name:ident { $($lit:literal => $num:literal),+ $(,)? }) => {
        pub fn $name(s: &str) -> Option<i32> {
            Some(match s {
                $($lit => $num,)+
                _ => return None,
            })
        }
    };
    ($name:ident + $rev:ident { $($lit:literal => $num:literal),+ $(,)? }) => {
        pub fn $name(s: &str) -> Option<i32> {
            Some(match s {
                $($lit => $num,)+
                _ => return None,
            })
        }
        pub fn $rev(v: i32) -> Option<&'static str> {
            Some(match v {
                $($num => $lit,)+
                _ => return None,
            })
        }
    };
}

mod audit;
mod authentication;
mod identity;
mod menu;
mod message;
mod monitor;
mod notification;
mod ops;
mod platform;

pub use audit::{
    data_access_sensitive_level_of, data_access_type_of, login_audit_action_of,
    login_audit_method_of, login_audit_risk_level_of, login_audit_status_of,
    operation_audit_action_of, operation_audit_sensitive_level_of, permission_audit_action_of,
};
pub use authentication::{
    login_policy_method_of, login_policy_method_str, login_policy_type_of, login_policy_type_str,
};
pub use identity::{
    org_unit_type_of, org_unit_type_str, plan_expiry_policy_of, plan_expiry_policy_str,
    plan_quota_type_of, plan_quota_type_str, plan_version_of, plan_version_str, position_type_of,
    role_scope_of, role_type_of, tenant_status_of, tenant_status_str, tenant_type_of,
    tenant_type_str, user_gender_of, user_status_of,
};
pub use menu::{menu_module_of, menu_module_str, menu_type_of, menu_type_str};
pub use message::{
    internal_message_recipient_status_of, internal_message_status_of, internal_message_type_of,
};
pub use monitor::{
    monitor_alert_op_of, monitor_alert_op_str, monitor_metric_of, monitor_metric_str,
};
pub use notification::{
    notification_channel_kind_of, notification_channel_kind_str, notification_channel_type_of,
    notification_channel_type_str, notification_delivery_status_of, notification_event_type_of,
    notification_event_type_str, notification_sign_style_of, notification_sign_style_str,
    notification_sms_provider_of, notification_sms_provider_str, notification_smtp_tls_of,
    notification_smtp_tls_str,
};
pub use ops::{script_language_of, script_language_str, task_type_of};
pub use platform::{config_value_type_of, file_provider_of};

#[cfg(test)]
mod tests {
    use super::*;

    /// Every both-way table must round-trip on its own pairs: the
    /// table is one bijection, so `str -> num -> str` and
    /// `num -> str -> num` are identities for every listed pair.
    #[test]
    fn both_way_tables_round_trip() {
        let both_way: Vec<(&str, Vec<(&str, i32)>)> = vec![
            (
                "menu_type",
                vec![("CATALOG", 0), ("BUTTON", 2), ("EMBEDDED", 3), ("LINK", 4)],
            ),
            (
                "menu_module",
                vec![
                    ("DASHBOARD", 1),
                    ("OPM", 2),
                    ("SYSTEM", 3),
                    ("DICT", 4),
                    ("TENANT", 5),
                    ("PERMISSION", 6),
                    ("LOG", 7),
                    ("INTERNAL_MESSAGE", 8),
                    ("FILE", 9),
                    ("TASK", 10),
                ],
            ),
            (
                "tenant_status",
                vec![("OFF", 1), ("EXPIRED", 2), ("FREEZE", 3)],
            ),
            (
                "tenant_type",
                vec![("TRIAL", 0), ("INTERNAL", 2), ("PARTNER", 3), ("CUSTOM", 4)],
            ),
            ("plan_version", vec![("STANDARD", 1), ("ENTERPRISE", 2)]),
            (
                "plan_expiry_policy",
                vec![("BLOCK_LOGIN", 1), ("FREEZE", 2)],
            ),
            ("plan_quota_type", vec![("STORAGE", 1), ("API_CALL", 2)]),
            (
                "org_unit_type",
                vec![
                    ("COMPANY", 1),
                    ("DIVISION", 2),
                    ("TEAM", 3),
                    ("PROJECT", 4),
                    ("COMMITTEE", 5),
                    ("REGION", 6),
                    ("OTHER", 7),
                ],
            ),
            ("script_language", vec![("JAVASCRIPT", 1)]),
            ("login_policy_type", vec![("WHITELIST", 2)]),
            (
                "login_policy_method",
                vec![("MAC", 2), ("REGION", 3), ("TIME", 4), ("DEVICE", 5)],
            ),
            (
                "notification_event_type",
                vec![
                    ("PASSWORD_RESET_CODE", 1),
                    ("CONTACT_BIND_CODE", 2),
                    ("CHANNEL_TEST_EMAIL", 3),
                    ("INTERNAL_MESSAGE", 4),
                ],
            ),
            (
                "notification_channel_kind",
                vec![("EMAIL", 1), ("SMS", 2), ("WEBHOOK", 3), ("INTERNAL", 4)],
            ),
            ("notification_smtp_tls", vec![("NONE", 0), ("SSL", 2)]),
            (
                "notification_sign_style",
                vec![("NONE", 1), ("DINGTALK", 2), ("FEISHU", 3), ("WECOM", 4)],
            ),
            (
                "notification_sms_provider",
                vec![("ALIYUN", 0), ("TENCENT", 1)],
            ),
            (
                "monitor_metric",
                vec![
                    ("GO_GOROUTINES", 1),
                    ("GO_MEM_ALLOC_MB", 2),
                    ("DB_OPEN_CONNECTIONS", 3),
                    ("DB_PING_FAIL", 4),
                    ("REDIS_DB_SIZE", 5),
                ],
            ),
            ("monitor_alert_op", vec![("GE", 1), ("LE", 2)]),
            (
                // 渠道 type 全表（老三样 + 全部 EXT_* 扩展名）双向对拍——
                // 39 个扩展名一个不落，编码与 proto enum Type 逐字一致。
                "notification_channel_type",
                vec![
                    ("EMAIL", 0),
                    ("WEBHOOK", 1),
                    ("SMS", 2),
                    ("WECOM_APP", 100),
                    ("DINGTALK_APP", 101),
                    ("FEISHU_APP", 102),
                    ("WECHAT", 103),
                    ("TELEGRAM", 104),
                    ("SLACK", 105),
                    ("DISCORD", 106),
                    ("TEAMS", 107),
                    ("GCHAT", 108),
                    ("WEBEX", 109),
                    ("LINE", 110),
                    ("MATRIX", 111),
                    ("MATTERMOST", 112),
                    ("ROCKETCHAT", 113),
                    ("ZULIP", 114),
                    ("BARK", 120),
                    ("NTFY", 121),
                    ("GOTIFY", 122),
                    ("SERVERCHAN", 123),
                    ("WXPUSHER", 124),
                    ("PUSHOVER", 125),
                    ("PUSHBULLET", 126),
                    ("PUSHDEER", 127),
                    ("JPUSH", 130),
                    ("GETUI", 131),
                    ("HUAWEISMS", 140),
                    ("YUNPIAN", 141),
                    ("TWILIO", 142),
                    ("VONAGE", 143),
                    ("WHATSAPP", 144),
                    ("MAILGUN", 150),
                    ("SENDGRID", 151),
                    ("POSTMARK", 152),
                    ("IFTTT", 160),
                    ("JOIN", 161),
                    ("HOMEASSISTANT", 162),
                    ("DATADOG", 170),
                    ("OPSGENIE", 171),
                    ("PAGERDUTY", 172),
                ],
            ),
        ];

        for (table, pairs) in both_way {
            for (lit, num) in &pairs {
                assert_eq!(str_to_num(table, lit), Some(*num), "{table}:{lit}");
                assert_eq!(num_to_str(table, *num), Some(*lit), "{table}:{num}");
            }
        }
    }

    /// The per-table dispatch oracles behind
    /// [`both_way_tables_round_trip`], keyed by name so the test data
    /// above reads as pure data.
    fn str_to_num(table: &str, lit: &str) -> Option<i32> {
        match table {
            "menu_type" => menu_type_of(lit),
            "menu_module" => menu_module_of(lit),
            "tenant_status" => tenant_status_of(lit),
            "tenant_type" => tenant_type_of(lit),
            "plan_version" => plan_version_of(lit),
            "plan_expiry_policy" => plan_expiry_policy_of(lit),
            "plan_quota_type" => plan_quota_type_of(lit),
            "org_unit_type" => org_unit_type_of(lit),
            "script_language" => script_language_of(lit),
            "login_policy_type" => login_policy_type_of(lit),
            "login_policy_method" => login_policy_method_of(lit),
            "notification_event_type" => notification_event_type_of(lit),
            "notification_channel_kind" => notification_channel_kind_of(lit),
            "notification_smtp_tls" => notification_smtp_tls_of(lit),
            "notification_sign_style" => notification_sign_style_of(lit),
            "notification_sms_provider" => notification_sms_provider_of(lit),
            "monitor_metric" => monitor_metric_of(lit),
            "monitor_alert_op" => monitor_alert_op_of(lit),
            "notification_channel_type" => notification_channel_type_of(lit),
            _ => unreachable!("unlisted both-way table {table}"),
        }
    }

    fn num_to_str(table: &str, num: i32) -> Option<&'static str> {
        match table {
            "menu_type" => menu_type_str(num),
            "menu_module" => menu_module_str(num),
            "tenant_status" => tenant_status_str(num),
            "tenant_type" => tenant_type_str(num),
            "plan_version" => plan_version_str(num),
            "plan_expiry_policy" => plan_expiry_policy_str(num),
            "plan_quota_type" => plan_quota_type_str(num),
            "org_unit_type" => org_unit_type_str(num),
            "script_language" => script_language_str(num),
            "login_policy_type" => login_policy_type_str(num),
            "login_policy_method" => login_policy_method_str(num),
            "notification_event_type" => notification_event_type_str(num),
            "notification_channel_kind" => notification_channel_kind_str(num),
            "notification_smtp_tls" => notification_smtp_tls_str(num),
            "notification_sign_style" => notification_sign_style_str(num),
            "notification_sms_provider" => notification_sms_provider_str(num),
            "monitor_metric" => monitor_metric_str(num),
            "monitor_alert_op" => monitor_alert_op_str(num),
            "notification_channel_type" => notification_channel_type_str(num),
            _ => unreachable!("unlisted both-way table {table}"),
        }
    }

    #[test]
    fn one_way_tables_answer_and_reject() {
        assert_eq!(user_status_of("CLOSED"), Some(9));
        assert_eq!(user_status_of("WHATEVER"), None);
        assert_eq!(operation_audit_action_of("CREATE"), Some(0));
        assert_eq!(operation_audit_action_of("NOPE"), None);
        assert_eq!(data_access_type_of("DDL_DROP"), Some(10));
        assert_eq!(login_audit_risk_level_of("HIGH"), Some(2));
        assert_eq!(task_type_of("DELAY"), Some(1));
        assert_eq!(config_value_type_of("STRING"), None);
    }
}
