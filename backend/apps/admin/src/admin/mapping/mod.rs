//! The DB status literals ↔ the wire enum numbers, one table per proto
//! enum. Both directions resolve from the contract's own descriptor
//! pool at first use — the tables cannot drift from the protos they
//! serve, and a proto value added later is mapped the moment the
//! binary is rebuilt. Domain fallbacks for unknown values stay at the
//! call sites (`.unwrap_or` with each domain's own default), so a
//! mismatched fallback pair can never be frozen into the table.
//!
//! `enum_map!($name => $ty)` emits `fn $name(&str) -> Option<i32>`;
//! `enum_map!($name + $rev => $ty)` additionally emits the inverse
//! `fn $rev(i32) -> Option<&'static str>`. `$ty` is the enum's fully
//! qualified proto name — a typo there surfaces as a `None` for every
//! value, which the module tests sweep for.
//!
//! The submodule layout is the wire axis: a submodule is named after
//! the proto package its enums ride in (a table shared by two packages
//! takes the neutral name). A newly ported domain's enums land in the
//! submodule of its proto package — no judgment call. Everything is
//! re-exported flat, so call sites keep reading
//! `crate::admin::mapping::<table>_of`.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The named proto enum's value name → number. `None` when the enum is
/// not in the pool or the text is not one of its value names.
pub(crate) fn enum_value_of(ty: &str, value: &str) -> Option<i32> {
    crate::pool()
        .get_enum_by_name(ty)?
        .values()
        .find(|v| v.name() == value)
        .map(|v| v.number())
}

/// The named proto enum's number → value name. The names must outlive
/// the call (they land in DB columns verbatim), so the pool's names
/// are leaked into a static table built on first use — the pool itself
/// is immortal (`crate::pool()` is a `OnceLock`), the leak only
/// duplicates the strings it already holds.
pub(crate) fn enum_name_of(ty: &'static str, number: i32) -> Option<&'static str> {
    static TABLE: OnceLock<HashMap<&'static str, HashMap<i32, &'static str>>> = OnceLock::new();
    TABLE
        .get_or_init(|| {
            let mut table: HashMap<&'static str, HashMap<i32, &'static str>> = HashMap::new();
            for ed in crate::pool().all_enums() {
                let name: &'static str = Box::leak(ed.full_name().to_string().into_boxed_str());
                let values = ed
                    .values()
                    .map(|v| {
                        let lit: &'static str = Box::leak(v.name().to_string().into_boxed_str());
                        (v.number(), lit)
                    })
                    .collect();
                table.insert(name, values);
            }
            table
        })
        .get(ty)?
        .get(&number)
        .copied()
}

/// One table per proto enum: the DB status literal ↔ the wire number.
///
/// * `$name => $ty` — emits `fn $name(&str) -> Option<i32>`;
/// * `$name + $rev => $ty` — additionally emits `fn $rev(i32) ->
///   Option<&'static str>`, backed by the same enum.
macro_rules! enum_map {
    ($name:ident => $ty:literal) => {
        pub fn $name(s: &str) -> Option<i32> {
            super::enum_value_of($ty, s)
        }
    };
    ($name:ident + $rev:ident => $ty:literal) => {
        pub fn $name(s: &str) -> Option<i32> {
            super::enum_value_of($ty, s)
        }
        pub fn $rev(v: i32) -> Option<&'static str> {
            super::enum_name_of($ty, v)
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
    position_type_str, role_scope_of, role_scope_str, role_type_of, role_type_str,
    tenant_audit_status_of, tenant_audit_status_str, tenant_status_of, tenant_status_str,
    tenant_type_of, tenant_type_str, user_gender_of, user_status_of,
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

    /// 每张表与其服务的 proto 枚举全名的绑定关系——表本体从描述符池
    /// 派生，这里的成对名单是测试的输入面：写错全名会让全值扫描全部
    /// 落 None 而炸出。
    fn served_enums() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "login_audit_action_of",
                "audit.service.v1.LoginAuditLog.ActionType",
            ),
            (
                "login_audit_status_of",
                "audit.service.v1.LoginAuditLog.Status",
            ),
            (
                "login_audit_method_of",
                "audit.service.v1.LoginAuditLog.LoginMethod",
            ),
            (
                "login_audit_risk_level_of",
                "audit.service.v1.LoginAuditLog.RiskLevel",
            ),
            (
                "operation_audit_action_of",
                "audit.service.v1.OperationAuditLog.ActionType",
            ),
            (
                "permission_audit_action_of",
                "audit.service.v1.PermissionAuditLog.ActionType",
            ),
            (
                "operation_audit_sensitive_level_of",
                "audit.service.v1.SensitiveLevel",
            ),
            (
                "data_access_type_of",
                "audit.service.v1.DataAccessAuditLog.AccessType",
            ),
            (
                "data_access_sensitive_level_of",
                "audit.service.v1.SensitiveLevel",
            ),
            (
                "login_policy_type_of",
                "authentication.service.v1.LoginPolicy.Type",
            ),
            (
                "login_policy_method_of",
                "authentication.service.v1.LoginPolicy.Method",
            ),
            ("user_gender_of", "identity.service.v1.User.Gender"),
            ("user_status_of", "identity.service.v1.User.Status"),
            ("role_scope_of", "identity.service.v1.DataScope"),
            ("role_type_of", "permission.service.v1.Role.Type"),
            ("position_type_of", "identity.service.v1.Position.Type"),
            ("tenant_status_of", "identity.service.v1.Tenant.Status"),
            ("tenant_type_of", "identity.service.v1.Tenant.Type"),
            ("plan_version_of", "identity.service.v1.Plan.Version"),
            (
                "plan_expiry_policy_of",
                "identity.service.v1.Plan.ExpiryPolicy",
            ),
            (
                "plan_quota_type_of",
                "identity.service.v1.PlanQuota.QuotaType",
            ),
            ("org_unit_type_of", "identity.service.v1.OrgUnit.Type"),
            ("menu_type_of", "permission.service.v1.Menu.Type"),
            ("menu_module_of", "identity.service.v1.Module"),
            (
                "internal_message_status_of",
                "internal_message.service.v1.InternalMessage.Status",
            ),
            (
                "internal_message_recipient_status_of",
                "internal_message.service.v1.InternalMessageRecipient.Status",
            ),
            (
                "internal_message_type_of",
                "internal_message.service.v1.InternalMessage.Type",
            ),
            (
                "monitor_metric_of",
                "monitor_alert.service.v1.MonitorMetric",
            ),
            ("monitor_alert_op_of", "monitor_alert.service.v1.AlertOp"),
            (
                "notification_event_type_of",
                "notification.service.v1.EventType",
            ),
            (
                "notification_channel_kind_of",
                "notification.service.v1.Channel",
            ),
            (
                "notification_channel_type_of",
                "notification_channel.service.v1.NotificationChannel.Type",
            ),
            (
                "notification_delivery_status_of",
                "notification.service.v1.DeliveryStatus",
            ),
            (
                "notification_smtp_tls_of",
                "notification_channel.service.v1.NotificationChannel.TlsMode",
            ),
            (
                "notification_sign_style_of",
                "notification_channel.service.v1.SignStyle",
            ),
            (
                "notification_sms_provider_of",
                "notification_channel.service.v1.SmsProvider",
            ),
            ("task_type_of", "task.service.v1.Task.Type"),
            ("script_language_of", "script.service.v1.Language"),
            (
                "config_value_type_of",
                "config.service.v1.Config.ConfigValueType",
            ),
            ("file_provider_of", "storage.service.v1.OSSProvider"),
        ]
    }

    /// 全值扫描：每张表必须把所服务枚举的每一个取值双向解析成功——
    /// 全名拼错（枚举不存在）或漏配都在这里炸出。断言走真实表函数
    /// （[`forward_of`]/[`reverse_of`]），表文件里的绑错全名无法被
    /// 绕过——错误全名让该表所有取值落 None，这里第一个值就炸。
    #[test]
    fn tables_resolve_every_proto_value_both_ways() {
        let pool = crate::pool();
        for (table, ty) in served_enums() {
            let ed = pool
                .get_enum_by_name(ty)
                .unwrap_or_else(|| panic!("enum not in pool: {table} -> {ty}"));
            // 哨兵号 -1：双向表回 Some(None)（函数存在但拒绝），单向表回
            // None（无反向函数）——以此分辨断言面。
            let has_reverse = reverse_of(table, -1).is_some();
            for v in ed.values() {
                assert_eq!(
                    forward_of(table, v.name()),
                    Some(v.number()),
                    "{table}: forward miss for {}",
                    v.name()
                );
                if has_reverse {
                    assert_eq!(
                        reverse_of(table, v.number()),
                        Some(Some(v.name())),
                        "{table}: reverse miss for {}",
                        v.name()
                    );
                }
            }
        }
    }

    /// 每张表的**函数本体**按表名调度——扫描测试的执行面；表文件里
    /// 的全名绑定在此处集中对拍。
    fn forward_of(table: &str, lit: &str) -> Option<i32> {
        match table {
            "login_audit_action_of" => login_audit_action_of(lit),
            "login_audit_status_of" => login_audit_status_of(lit),
            "login_audit_method_of" => login_audit_method_of(lit),
            "login_audit_risk_level_of" => login_audit_risk_level_of(lit),
            "operation_audit_action_of" => operation_audit_action_of(lit),
            "permission_audit_action_of" => permission_audit_action_of(lit),
            "operation_audit_sensitive_level_of" => operation_audit_sensitive_level_of(lit),
            "data_access_type_of" => data_access_type_of(lit),
            "data_access_sensitive_level_of" => data_access_sensitive_level_of(lit),
            "login_policy_type_of" => login_policy_type_of(lit),
            "login_policy_method_of" => login_policy_method_of(lit),
            "user_gender_of" => user_gender_of(lit),
            "user_status_of" => user_status_of(lit),
            "role_scope_of" => role_scope_of(lit),
            "role_type_of" => role_type_of(lit),
            "position_type_of" => position_type_of(lit),
            "tenant_status_of" => tenant_status_of(lit),
            "tenant_type_of" => tenant_type_of(lit),
            "plan_version_of" => plan_version_of(lit),
            "plan_expiry_policy_of" => plan_expiry_policy_of(lit),
            "plan_quota_type_of" => plan_quota_type_of(lit),
            "org_unit_type_of" => org_unit_type_of(lit),
            "menu_type_of" => menu_type_of(lit),
            "menu_module_of" => menu_module_of(lit),
            "internal_message_status_of" => internal_message_status_of(lit),
            "internal_message_recipient_status_of" => internal_message_recipient_status_of(lit),
            "internal_message_type_of" => internal_message_type_of(lit),
            "monitor_metric_of" => monitor_metric_of(lit),
            "monitor_alert_op_of" => monitor_alert_op_of(lit),
            "notification_event_type_of" => notification_event_type_of(lit),
            "notification_channel_kind_of" => notification_channel_kind_of(lit),
            "notification_channel_type_of" => notification_channel_type_of(lit),
            "notification_delivery_status_of" => notification_delivery_status_of(lit),
            "notification_smtp_tls_of" => notification_smtp_tls_of(lit),
            "notification_sign_style_of" => notification_sign_style_of(lit),
            "notification_sms_provider_of" => notification_sms_provider_of(lit),
            "task_type_of" => task_type_of(lit),
            "script_language_of" => script_language_of(lit),
            "config_value_type_of" => config_value_type_of(lit),
            "file_provider_of" => file_provider_of(lit),
            _ => unreachable!("unlisted table {table}"),
        }
    }

    /// 反向函数调度：`None` 外层 = 该表是单向表（无反向函数）。
    fn reverse_of(table: &str, num: i32) -> Option<Option<&'static str>> {
        match table {
            "login_policy_type_of" => Some(login_policy_type_str(num)),
            "login_policy_method_of" => Some(login_policy_method_str(num)),
            "role_scope_of" => Some(role_scope_str(num)),
            "role_type_of" => Some(role_type_str(num)),
            "position_type_of" => Some(position_type_str(num)),
            "tenant_status_of" => Some(tenant_status_str(num)),
            "tenant_type_of" => Some(tenant_type_str(num)),
            "plan_version_of" => Some(plan_version_str(num)),
            "plan_expiry_policy_of" => Some(plan_expiry_policy_str(num)),
            "plan_quota_type_of" => Some(plan_quota_type_str(num)),
            "org_unit_type_of" => Some(org_unit_type_str(num)),
            "menu_type_of" => Some(menu_type_str(num)),
            "menu_module_of" => Some(menu_module_str(num)),
            "monitor_metric_of" => Some(monitor_metric_str(num)),
            "monitor_alert_op_of" => Some(monitor_alert_op_str(num)),
            "notification_event_type_of" => Some(notification_event_type_str(num)),
            "notification_channel_kind_of" => Some(notification_channel_kind_str(num)),
            "notification_channel_type_of" => Some(notification_channel_type_str(num)),
            "notification_smtp_tls_of" => Some(notification_smtp_tls_str(num)),
            "notification_sign_style_of" => Some(notification_sign_style_str(num)),
            "notification_sms_provider_of" => Some(notification_sms_provider_str(num)),
            "script_language_of" => Some(script_language_str(num)),
            _ => None,
        }
    }

    /// 此前手写表整体错位一格的取值（去掉 UNSPECIFIED 后从 1 数）按
    /// proto 编号重新钉死——漂移回归的第一现场。
    #[test]
    fn previously_drifted_tables_pin_the_proto_numbers() {
        assert_eq!(login_audit_action_of("LOGIN"), Some(1));
        assert_eq!(login_audit_status_of("SUCCESS"), Some(1));
        assert_eq!(login_audit_method_of("PASSWORD"), Some(1));
        assert_eq!(login_audit_risk_level_of("LOW"), Some(1));
        assert_eq!(operation_audit_action_of("CREATE"), Some(1));
        assert_eq!(operation_audit_action_of("OTHER"), Some(100));
        assert_eq!(permission_audit_action_of("GRANT"), Some(1));
        assert_eq!(permission_audit_action_of("OTHER"), Some(100));
        assert_eq!(operation_audit_sensitive_level_of("PUBLIC"), Some(1));
        assert_eq!(operation_audit_sensitive_level_of("SECRET"), Some(4));
        assert_eq!(data_access_type_of("SELECT"), Some(1));
        assert_eq!(data_access_type_of("OTHER"), Some(100));
        assert_eq!(role_scope_of("ALL"), Some(1));
        assert_eq!(role_scope_of("SELECTED_UNITS"), Some(5));
        assert_eq!(role_type_of("SYSTEM"), Some(0));
        assert_eq!(role_type_of("TENANT"), Some(2));
        assert_eq!(position_type_of("REGULAR"), Some(0));
        assert_eq!(position_type_of("LEADER"), Some(1));
        assert_eq!(position_type_of("OTHER"), Some(100));
        assert_eq!(tenant_status_of("ON"), Some(1));
        assert_eq!(tenant_status_of("OFF"), Some(0));
        assert_eq!(tenant_type_of("TRIAL"), Some(1));
        assert_eq!(tenant_type_of("PAID"), Some(2));
        assert_eq!(plan_version_of("FREE"), Some(1));
        assert_eq!(plan_version_of("STANDARD"), Some(2));
        assert_eq!(plan_expiry_policy_of("READONLY"), Some(1));
        assert_eq!(plan_expiry_policy_of("FREEZE"), Some(3));
        assert_eq!(org_unit_type_of("COMPANY"), Some(0));
        assert_eq!(org_unit_type_of("BRANCH"), Some(8));
        assert_eq!(org_unit_type_of("OTHER"), Some(100));
        assert_eq!(menu_type_of("MENU"), Some(1));
        assert_eq!(internal_message_status_of("ARCHIVED"), Some(5));
        assert_eq!(internal_message_status_of("DELETED"), Some(6));
        assert_eq!(file_provider_of("MINIO"), Some(0));
        assert_eq!(file_provider_of("LOCAL"), Some(10));
        assert_eq!(config_value_type_of("STRING"), Some(1));
        assert_eq!(login_policy_type_of("BLACKLIST"), Some(1));
        assert_eq!(login_policy_method_of("IP"), Some(1));
        assert_eq!(notification_smtp_tls_of("START_TLS"), Some(1));
        assert_eq!(notification_channel_kind_of("EXT"), Some(5));
    }

    /// 非取值名照旧落 None——域内兜底仍在调用侧。
    #[test]
    fn unknown_values_still_reject() {
        assert_eq!(role_scope_of("WHATEVER"), None);
        assert_eq!(role_scope_str(-1), None);
        assert_eq!(user_status_of("CLOSED"), Some(9));
        assert_eq!(user_status_of("NOPE"), None);
    }
}
