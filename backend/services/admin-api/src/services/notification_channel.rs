//! NotificationChannelService — SMTP/webhook/SMS/EXT 渠道 CRUD + 测试发送 +
//! 扩展渠道 schema 注册表面。
//!
//! 出站投递（SMTP/WEBHOOK/SMS/EXT sender + 台账）仍随出站里程碑；SendTest
//! 现阶段按上游 Precheck 语义做"配置在场"校验（不拨号）。

use std::collections::BTreeMap;
use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::mapping;
use crate::state::{db_err, operator_of, require_data, status_error, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::notification_channel::service::v1::ChannelSchema;
use proto::proto::notification_channel::service::v1::{
    CreateNotificationChannelRequest, DeleteNotificationChannelRequest,
    GetNotificationChannelRequest, ListChannelSchemasResponse, ListNotificationChannelResponse,
    NotificationChannel, SendTestEmailRequest, SendTestRequest, UpdateNotificationChannelRequest,
};
use proto::proto::pagination::PagingRequest;

use super::channel_schemas;

/// 类型是否属于扩展渠道（settings 整包面；老三样 EMAIL/WEBHOOK/SMS 之外）。
fn is_ext_type(type_name: &str) -> bool {
    !matches!(type_name, "EMAIL" | "WEBHOOK" | "SMS")
}

/// 按类型名取 schema（schema 注册表 39 条线性扫，量小不值得建索引）。
fn schema_by_type(type_name: &str) -> Option<ChannelSchema> {
    channel_schemas::schemas()
        .into_iter()
        .find(|s| s.r#type == type_name)
}

fn channel_proto(r: crate::data::sys_notification_channels::Model) -> NotificationChannel {
    let type_name = r.type_column.clone().unwrap_or_default();
    NotificationChannel {
        id: Some(r.id),
        name: Some(r.name),
        r#type: mapping::notification_channel_type_of(&type_name),
        smtp_host: r.smtp_host,
        smtp_port: r.smtp_port,
        smtp_username: r.smtp_username,
        has_password: r
            .smtp_password
            .as_deref()
            .is_some_and(|p| !p.is_empty())
            .then_some(true),
        smtp_from: r.smtp_from,
        // Unknown rows read as START_TLS.
        smtp_tls: r
            .smtp_tls
            .as_deref()
            .map(|s| mapping::notification_smtp_tls_of(s).unwrap_or(1)),
        enabled: r.status.as_deref().map(|s| s != "OFF"),
        remark: r.remark,
        created_by: r.created_by,
        updated_by: r.updated_by,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        webhook_url: r.webhook_url,
        has_webhook_secret: r
            .webhook_secret
            .as_deref()
            .is_some_and(|s| !s.is_empty())
            .then_some(true),
        // Unknown rows read as CUSTOM.
        webhook_sign_style: r
            .webhook_sign_style
            .as_deref()
            .map(|s| mapping::notification_sign_style_of(s).unwrap_or(0)),
        webhook_payload_template: r.webhook_payload_template,
        // 回显脱敏：settings 里 sensitive 字段恒空串（前端"留空 = 不修改"）。
        settings: Some(sanitize_settings_for_dto(&type_name, r.settings.as_deref())),
        sms_provider: r
            .sms_provider
            .as_deref()
            .and_then(mapping::notification_sms_provider_of),
        sms_access_key_id: r.sms_access_key_id,
        has_sms_access_secret: r
            .sms_access_secret
            .as_deref()
            .is_some_and(|s| !s.is_empty())
            .then_some(true),
        sms_sign_name: r.sms_sign_name,
        sms_template_code: r.sms_template_code,
        sms_app_id: r.sms_app_id,
        sms_region: r.sms_region,
        smtp_from_name: r.smtp_from_name,
    }
}

pub struct NotificationChannelService {
    pub state: Arc<AppState>,
}

/// settings 写入规范化：明文 JSON 按 schema 校验必填（值取字符串形态，
/// 数字/布尔按上游 settingsMap.str 的宽松读法归一）。敏感字段加密随出站
/// 投递里程碑，存储态暂为校验后的明文——回显侧已按 sensitive 脱敏兜底。
fn normalize_settings_for_store(type_name: &str, plaintext: &str) -> Result<String, StatusError> {
    let Some(schema) = schema_by_type(type_name) else {
        return Err(status_error(
            "BAD_REQUEST",
            "settings only applies to EXT_* channel types",
        ));
    };
    if plaintext.trim().is_empty() {
        return Err(status_error("BAD_REQUEST", "settings is empty"));
    }
    let raw: BTreeMap<String, serde_json::Value> = serde_json::from_str(plaintext)
        .map_err(|_| status_error("BAD_REQUEST", "settings is not valid JSON"))?;
    let as_str = |v: Option<&serde_json::Value>| -> String {
        match v {
            Some(serde_json::Value::String(s)) => s.trim().to_string(),
            Some(serde_json::Value::Number(n)) => n.to_string(),
            Some(serde_json::Value::Bool(b)) => b.to_string(),
            _ => String::new(),
        }
    };
    for f in &schema.fields {
        if f.required && as_str(raw.get(&f.name)).is_empty() {
            return Err(status_error(
                "BAD_REQUEST",
                format!("{} is required", f.label),
            ));
        }
    }
    serde_json::to_string(&raw)
        .map_err(|_| status_error("BAD_REQUEST", "settings is not valid JSON"))
}

/// 读回显脱敏：settings 里 sensitive 字段值恒为空串；非 JSON 存储态原样返回
/// （老数据防御）。非扩展渠道无 settings 面，返回空串。
fn sanitize_settings_for_dto(type_name: &str, store: Option<&str>) -> String {
    let Some(store) = store.filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let Some(schema) = schema_by_type(type_name) else {
        return String::new();
    };
    let Ok(mut raw) = serde_json::from_str::<BTreeMap<String, serde_json::Value>>(store) else {
        return String::new();
    };
    for f in &schema.fields {
        if f.sensitive {
            raw.insert(f.name.clone(), serde_json::Value::String(String::new()));
        }
    }
    serde_json::to_string(&raw).unwrap_or_default()
}

/// settings 更新合并：sensitive 字段留空 = 保留已存值；随后按 schema 校验。
/// 存储态与传入态都按"脱敏视角"处理——调用方给的是回显形态。
fn merge_settings_for_update(
    type_name: &str,
    stored: Option<&str>,
    incoming: &str,
) -> Result<String, StatusError> {
    let schema = schema_by_type(type_name).ok_or_else(|| {
        status_error(
            "BAD_REQUEST",
            "settings only applies to EXT_* channel types",
        )
    })?;
    let mut merged: BTreeMap<String, serde_json::Value> = stored
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    let incoming: BTreeMap<String, serde_json::Value> = serde_json::from_str(incoming)
        .map_err(|_| status_error("BAD_REQUEST", "settings is not valid JSON"))?;
    for f in &schema.fields {
        let Some(v) = incoming.get(&f.name) else {
            continue;
        };
        let empty_plain = matches!(v, serde_json::Value::String(s) if s.trim().is_empty());
        if f.sensitive && empty_plain {
            continue; // 留空 = 保留已存密文/明文
        }
        merged.insert(f.name.clone(), v.clone());
    }
    let merged_str =
        serde_json::to_string(&merged).map_err(|_| status_error("BAD_REQUEST", "settings"))?;
    normalize_settings_for_store(type_name, &merged_str)
}

#[async_trait::async_trait]
impl proto::gen::services::NotificationChannelServiceHandlers for NotificationChannelService {
    async fn list_notification_channel(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListNotificationChannelResponse, StatusError> {
        let repo = crate::data::repos::NotificationChannelRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListNotificationChannelResponse {
            items: rows.into_iter().map(channel_proto).collect(),
            total,
        })
    }

    async fn get_notification_channel(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetNotificationChannelRequest,
    ) -> Result<NotificationChannel, StatusError> {
        let repo = crate::data::repos::NotificationChannelRepo::new(&self.state.db);
        Ok(channel_proto(repo.get_by_id(req.id).await?))
    }

    async fn create_notification_channel(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateNotificationChannelRequest,
    ) -> Result<NotificationChannel, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let type_name = data
            .r#type
            .and_then(mapping::notification_channel_type_str)
            .unwrap_or("EMAIL");
        let settings = match data.settings.as_deref().filter(|s| !s.is_empty()) {
            Some(s) => Some(normalize_settings_for_store(type_name, s)?),
            None => {
                if is_ext_type(type_name) {
                    // 扩展渠道行必须带配置整包，否则 Precheck/出站无从谈起。
                    return Err(status_error("BAD_REQUEST", "settings is empty"));
                }
                None
            }
        };
        let inserted = crate::data::sys_notification_channels::ActiveModel {
            name: Set(data.name.unwrap_or_default()),
            type_column: Set(Some(type_name.into())),
            smtp_host: Set(data.smtp_host),
            smtp_port: Set(data.smtp_port),
            smtp_username: Set(data.smtp_username),
            smtp_password: Set(None), // secrets write via update with a password payload
            smtp_from: Set(data.smtp_from),
            smtp_tls: Set(Some(
                data.smtp_tls
                    .map_or("START_TLS", |v| {
                        mapping::notification_smtp_tls_str(v).unwrap_or("START_TLS")
                    })
                    .to_string(),
            )),
            webhook_url: Set(data.webhook_url),
            webhook_secret: Set(None), // secrets write via update with a secret payload
            webhook_sign_style: Set(data.webhook_sign_style.map(|v| {
                mapping::notification_sign_style_str(v)
                    .unwrap_or("CUSTOM")
                    .to_string()
            })),
            webhook_payload_template: Set(data.webhook_payload_template),
            settings: Set(settings),
            sms_provider: Set(data
                .sms_provider
                .and_then(mapping::notification_sms_provider_str)
                .map(|s| s.to_string())),
            sms_access_key_id: Set(data.sms_access_key_id),
            sms_access_secret: Set(None), // has 布尔回显；明文写入口随出站里程碑
            sms_sign_name: Set(data.sms_sign_name),
            sms_template_code: Set(data.sms_template_code),
            sms_app_id: Set(data.sms_app_id),
            sms_region: Set(data.sms_region),
            smtp_from_name: Set(data.smtp_from_name),
            status: Set(Some("ON".into())),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        Ok(channel_proto(inserted))
    }

    async fn update_notification_channel(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateNotificationChannelRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let repo = crate::data::repos::NotificationChannelRepo::new(&self.state.db);
        let row = repo.get_by_id(req.id).await?;
        let type_name = row.type_column.clone().unwrap_or_default();
        let mut a: crate::data::sys_notification_channels::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.name {
                a.name = Set(v.clone());
            }
            if let Some(v) = &data.smtp_host {
                a.smtp_host = Set(Some(v.clone()));
            }
            if let Some(v) = data.smtp_port {
                a.smtp_port = Set(Some(v));
            }
            if let Some(v) = &data.smtp_username {
                a.smtp_username = Set(Some(v.clone()));
            }
            if let Some(v) = &data.smtp_from {
                a.smtp_from = Set(Some(v.clone()));
            }
            if let Some(v) = &data.webhook_url {
                a.webhook_url = Set(Some(v.clone()));
            }
            if let Some(v) = &data.webhook_payload_template {
                a.webhook_payload_template = Set(Some(v.clone()));
            }
            if let Some(v) = data.webhook_sign_style {
                a.webhook_sign_style = Set(Some(
                    mapping::notification_sign_style_str(v)
                        .unwrap_or("CUSTOM")
                        .to_string(),
                ));
            }
            if let Some(v) = data.smtp_tls {
                a.smtp_tls = Set(Some(
                    mapping::notification_smtp_tls_str(v)
                        .unwrap_or("START_TLS")
                        .to_string(),
                ));
            }
            if let Some(v) = data.enabled {
                a.status = Set(Some(if v { "ON".into() } else { "OFF".into() }));
            }
            // 扩展渠道 settings 合并（sensitive 留空 = 保留已存）；非扩展渠道
            // 传 settings 一律拒绝（与上游 normalizeSettingsForWrite 同则）。
            if let Some(v) = data.settings.as_deref().filter(|s| !s.is_empty()) {
                let stored = {
                    let sea_orm::ActiveValue::Unchanged(current) = &a.settings else {
                        unreachable!("freshly loaded row carries Unchanged values")
                    };
                    current.clone()
                };
                let merged = merge_settings_for_update(&type_name, stored.as_deref(), v)?;
                a.settings = Set(Some(merged));
            }
            if let Some(v) = data.sms_provider {
                a.sms_provider =
                    Set(mapping::notification_sms_provider_str(v).map(|s| s.to_string()));
            }
            if let Some(v) = &data.sms_access_key_id {
                a.sms_access_key_id = Set(Some(v.clone()));
            }
            if let Some(v) = &data.sms_sign_name {
                a.sms_sign_name = Set(Some(v.clone()));
            }
            if let Some(v) = &data.sms_template_code {
                a.sms_template_code = Set(Some(v.clone()));
            }
            if let Some(v) = &data.sms_app_id {
                a.sms_app_id = Set(Some(v.clone()));
            }
            if let Some(v) = &data.sms_region {
                a.sms_region = Set(Some(v.clone()));
            }
            if let Some(v) = &data.smtp_from_name {
                a.smtp_from_name = Set(Some(v.clone()));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete_notification_channel(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteNotificationChannelRequest,
    ) -> Result<Empty, StatusError> {
        let repo = crate::data::repos::NotificationChannelRepo::new(&self.state.db);
        repo.get_by_id(req.id).await?;
        repo.delete_by_id(req.id).await?;
        Ok(Empty {})
    }

    async fn send_test_email(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: SendTestEmailRequest,
    ) -> Result<Empty, StatusError> {
        // Delivery rides the mailer phase; validate the channel exists.
        let repo = crate::data::repos::NotificationChannelRepo::new(&self.state.db);
        let row = repo.get_by_id(req.id).await?;
        if row.smtp_host.as_deref().map_or(true, |h| h.is_empty()) {
            return Err(status_error(
                "BAD_REQUEST",
                "channel has no smtp host configured",
            ));
        }
        Ok(Empty {})
    }

    async fn send_test(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: SendTestRequest,
    ) -> Result<Empty, StatusError> {
        // 两代测试入口共同语义：id/recipient 必填 → 查类型 → 配置在场校验
        // （上游 Precheck：解配置并按 schema 校验必填，不拨号）。实际投递
        // 与台账随出站里程碑接入。
        if req.id == 0 {
            return Err(status_error("BAD_REQUEST", "id is required"));
        }
        if req.recipient.trim().is_empty() {
            return Err(status_error("BAD_REQUEST", "recipient is required"));
        }
        let repo = crate::data::repos::NotificationChannelRepo::new(&self.state.db);
        let row = repo.get_by_id(req.id).await?;
        let type_name = row.type_column.as_deref().unwrap_or("");
        let configured = match type_name {
            "EMAIL" => row.smtp_host.as_deref().is_some_and(|h| !h.is_empty()),
            "WEBHOOK" => row.webhook_url.as_deref().is_some_and(|u| !u.is_empty()),
            "SMS" => {
                row.sms_access_key_id
                    .as_deref()
                    .is_some_and(|k| !k.is_empty())
                    && row
                        .sms_access_secret
                        .as_deref()
                        .is_some_and(|s| !s.is_empty())
            }
            other if is_ext_type(other) => {
                let settings = row.settings.as_deref().unwrap_or("");
                let parsed: BTreeMap<String, serde_json::Value> =
                    serde_json::from_str(settings).unwrap_or_default();
                let as_str = |v: Option<&serde_json::Value>| -> String {
                    match v {
                        Some(serde_json::Value::String(s)) => s.trim().to_string(),
                        Some(serde_json::Value::Number(n)) => n.to_string(),
                        Some(serde_json::Value::Bool(b)) => b.to_string(),
                        _ => String::new(),
                    }
                };
                schema_by_type(type_name).is_some_and(|schema| {
                    schema
                        .fields
                        .iter()
                        .all(|f| !f.required || !as_str(parsed.get(&f.name)).is_empty())
                })
            }
            _ => false,
        };
        if !configured {
            return Err(status_error(
                "BAD_REQUEST",
                "channel is not fully configured for its type",
            ));
        }
        Ok(Empty {})
    }

    async fn list_channel_schemas(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<ListChannelSchemasResponse, StatusError> {
        // 扩展渠道 schema 注册表（39 个，前端动态表单数据源）。
        Ok(ListChannelSchemasResponse {
            items: channel_schemas::schemas(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_write_requires_schema_required_fields() {
        // WECOM_APP：corpId/corpSecret/agentId 必填
        let ok = r#"{"corpId":"wwe8","corpSecret":"s3cret","agentId":1000002}"#;
        assert!(normalize_settings_for_store("WECOM_APP", ok).is_ok());
        let missing = r#"{"corpId":"wwe8"}"#;
        let err = normalize_settings_for_store("WECOM_APP", missing).unwrap_err();
        assert!(err.message.contains("required"), "{err:?}");
        // 非 EXT 渠道不允许 settings
        assert!(normalize_settings_for_store("EMAIL", ok).is_err());
        // 坏 JSON
        assert!(normalize_settings_for_store("WECOM_APP", "{not json").is_err());
    }

    #[test]
    fn settings_read_masks_sensitive_fields() {
        let store = r#"{"corpId":"wwe8","corpSecret":"s3cret","agentId":1000002}"#;
        let dto = sanitize_settings_for_dto("WECOM_APP", Some(store));
        assert!(dto.contains("wwe8"));
        assert!(!dto.contains("s3cret"), "sensitive must mask: {dto}");
        assert!(dto.contains("\"corpSecret\":\"\""));
        // 非 EXT / 空存储 → 空串
        assert_eq!(sanitize_settings_for_dto("EMAIL", Some(store)), "");
        assert_eq!(sanitize_settings_for_dto("WECOM_APP", Some("")), "");
    }

    #[test]
    fn settings_update_merge_keeps_stored_secret_on_empty() {
        let stored = r#"{"corpId":"wwe8","corpSecret":"s3cret","agentId":1000002}"#;
        // 回显形态回传（corpSecret 空串）→ 保留已存
        let incoming = r#"{"corpId":"wwe9","corpSecret":"","agentId":1000003}"#;
        let merged = merge_settings_for_update("WECOM_APP", Some(stored), incoming).unwrap();
        assert!(merged.contains("\"corpId\":\"wwe9\""));
        assert!(
            merged.contains("s3cret"),
            "stored secret must survive: {merged}"
        );
        assert!(merged.contains("1000003"));
        // 非空新值 → 覆盖
        let incoming2 = r#"{"corpSecret":"new-secret"}"#;
        let merged2 = merge_settings_for_update("WECOM_APP", Some(stored), incoming2).unwrap();
        assert!(merged2.contains("new-secret"));
        assert!(!merged2.contains("s3cret"));
    }

    #[test]
    fn registry_has_all_39_schemas_with_proto_codes() {
        let all = channel_schemas::schemas();
        assert_eq!(all.len(), 39);
        // 每条 schema 的类型名都能映射回 proto Type 枚举码，且是 EXT 面
        for s in &all {
            let code = mapping::notification_channel_type_of(&s.r#type)
                .unwrap_or_else(|| panic!("schema type {} missing proto code", s.r#type));
            assert!(code >= 100, "{s:?} should be an EXT code");
            assert!(!s.label.is_empty());
        }
        // 抽查一条声明保真（IM 首条）
        let wecom = all.iter().find(|s| s.r#type == "WECOM_APP").unwrap();
        assert_eq!(wecom.group, "IM");
        assert_eq!(wecom.label, "企业微信应用消息");
        assert!(wecom.needs_target);
        assert_eq!(wecom.fields.len(), 4);
        assert!(wecom.fields[1].sensitive);
    }
}
