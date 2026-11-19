//! AiProviderService — LLM 提供商 CRUD。api_key 明文经全局加密器
//! （AES-256-GCM，enc: 前缀）落库并刷新脱敏 hint；更新时 key 为空 = 本次
//! 不改 key（hint 与 has 面照常回显），与上游 service 层语义一致。
//! is_default 唯一化：置默认时清掉同租户其他默认行。
//! 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即存量
//! 全量提交语义）——密钥与唯一化的特殊逻辑原位保留，写面同样过门。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::admin::data::Viewer;
use crate::admin::state::{db_err, not_found, operator_of, require_data, AppState, StatusError};
use crate::proto::ai::service::v1::{
    AiProvider, CreateAiProviderRequest, DeleteAiProviderRequest, GetAiProviderRequest,
    ListAiProviderResponse, UpdateAiProviderRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

/// ModelType proto 枚举 ↔ ent 列值（与上游枚举名逐字一致）。
fn model_type_str(v: i32) -> Option<&'static str> {
    Some(match v {
        1 => "OPENAI",
        2 => "AZURE_OPENAI",
        3 => "OLLAMA",
        4 => "OPENAI_COMPATIBLE",
        5 => "MOONSHOT",
        6 => "ZHIPU_AI",
        7 => "QWEN",
        8 => "DEEPSEEK",
        _ => return None,
    })
}

fn model_type_proto(s: &str) -> i32 {
    match s {
        "OPENAI" => 1,
        "AZURE_OPENAI" => 2,
        "OLLAMA" => 3,
        "OPENAI_COMPATIBLE" => 4,
        "MOONSHOT" => 5,
        "ZHIPU_AI" => 6,
        "QWEN" => 7,
        "DEEPSEEK" => 8,
        _ => 0,
    }
}

fn hint_of(key: &str) -> String {
    let t = key.trim();
    if t.is_empty() {
        return String::new();
    }
    match t.len() {
        n if n <= 8 => format!("sk-***{t}"),
        _ => format!("sk-***{}", &t[t.len() - 4..]),
    }
}

fn provider_proto(r: crate::admin::data::sys_ai_providers::Model) -> AiProvider {
    AiProvider {
        id: Some(r.id as u64),
        name: r.name,
        model_type: r.model_type.as_deref().map(model_type_proto),
        model_name: r.model_name,
        base_url: r.base_url,
        organization: r.organization,
        // api_key 明文永不出库（proto 字段仅承载写入载荷）；hint 回显。
        api_key: String::new().into(),
        api_key_hint: r.api_key_hint,
        local_host: r.local_host,
        local_port: r.local_port,
        timeout_seconds: r.timeout_seconds,
        system_prompt: r.system_prompt,
        is_default: r.is_default,
        remark: r.remark,
        is_enabled: r.is_enabled,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
    }
}

/// 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即存量
/// if-let-Some 全量提交语义：None 不动列）。api_key 加密落库/hint 刷新
/// 原位保留，但写面同样过门；model_type 未知枚举值沿用"整段跳过不写"
/// （opt 的 None 清列通道表达不了这个语义，留 if-let 走门）。
fn apply_fields(
    a: &mut crate::data::sys_ai_providers::ActiveModel,
    data: &AiProvider,
    mask: &crate::fieldmask::FieldMask,
) {
    crate::set_masked!(opt mask, a, name, data.name, |v: &String| Some(v.clone()));
    if mask.touches("model_type") {
        if let Some(v) = data.model_type.and_then(model_type_str) {
            a.model_type = Set(Some(v.into()));
        }
    }
    crate::set_masked!(opt mask, a, model_name, data.model_name, |v: &String| Some(
        v.clone()
    ));
    crate::set_masked!(opt mask, a, base_url, data.base_url, |v: &String| Some(
        v.clone()
    ));
    crate::set_masked!(opt mask, a, organization, data.organization, |v: &String| Some(
        v.clone()
    ));
    crate::set_masked!(opt mask, a, local_host, data.local_host, |v: &String| Some(
        v.clone()
    ));
    crate::set_masked!(opt mask, a, local_port, data.local_port, |v: &i32| Some(*v));
    crate::set_masked!(opt mask, a, timeout_seconds, data.timeout_seconds, |v: &i32| Some(
        *v
    ));
    crate::set_masked!(opt mask, a, system_prompt, data.system_prompt, |v: &String| Some(
        v.clone()
    ));
    crate::set_masked!(opt mask, a, is_default, data.is_default, |v: &bool| Some(*v));
    crate::set_masked!(opt mask, a, remark, data.remark, |v: &String| Some(v.clone()));
    crate::set_masked!(opt mask, a, is_enabled, data.is_enabled, |v: &bool| Some(*v));
    // api_key 明文非空 → 加密落库并刷新 hint；空 = 本次不改 key（hint
    // 与 has 面照常回显）——密钥面特殊逻辑，掩码未选中时密钥不动。
    if mask.touches("api_key") {
        if let Some(k) = data
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
        {
            a.api_key = Set(Some(crate::crypto::encrypt_if_needed(k)));
            a.api_key_hint = Set(Some(hint_of(k)));
        }
    }
}

pub struct AiProviderService {
    pub state: Arc<AppState>,
}

impl AiProviderService {
    async fn clear_other_defaults(&self, tenant_id: i64, keep_id: i64) -> Result<(), StatusError> {
        let rows = crate::admin::data::sys_ai_providers::Entity::find()
            .filter(
                sea_orm::Condition::all()
                    .add(crate::admin::data::sys_ai_providers::Column::TenantId.eq(tenant_id))
                    .add(crate::admin::data::sys_ai_providers::Column::IsDefault.eq(true)),
            )
            .all(&self.state.db)
            .await
            .map_err(db_err)?;
        for row in rows {
            if row.id == keep_id {
                continue;
            }
            let mut a: crate::admin::data::sys_ai_providers::ActiveModel = row.into();
            a.is_default = Set(Some(false));
            a.update(&self.state.db).await.map_err(db_err)?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::proto::gen::services::AiProviderServiceHandlers for AiProviderService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListAiProviderResponse, StatusError> {
        let repo =
            crate::admin::data::repos::AiProviderRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListAiProviderResponse {
            items: rows.into_iter().map(provider_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetAiProviderRequest,
    ) -> Result<AiProvider, StatusError> {
        let repo =
            crate::admin::data::repos::AiProviderRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::ai::service::v1::get_ai_provider_request::QueryBy
        );
        let row = repo
            .find_scoped(id as i64)
            .await?
            .ok_or_else(|| not_found("ai provider"))?;
        Ok(provider_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateAiProviderRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let _repo = crate::data::repos::AiProviderRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let mut a = crate::data::sys_ai_providers::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        };
        // 无掩码全放行：api_key 的加密落库/hint 刷新统一在 apply_fields。
        apply_fields(&mut a, &data, &crate::fieldmask::FieldMask::unmasked());
        if a.is_enabled.clone().unwrap().is_none() {
            a.is_enabled = Set(Some(true));
        }
        let inserted = a.insert(&self.state.db).await.map_err(db_err)?;
        if inserted.is_default == Some(true) {
            self.clear_other_defaults(payload.tenant_id, inserted.id as u64 as i64)
                .await?;
        }
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateAiProviderRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo =
            crate::admin::data::repos::AiProviderRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = repo
            .find_scoped(req.id as i64)
            .await?
            .ok_or_else(|| not_found("ai provider"))?;
        let mut a: crate::data::sys_ai_providers::ActiveModel = row.into();
        // 字段级更新：掩码白名单外字段不动（无掩码 = 全量提交语义不变）；
        // key 为空 → 本次请求不改 key，api_key 的加密写面在 apply_fields
        // 内过门。
        let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
        apply_fields(&mut a, &data, &mask);
        crate::stamp_update!(a, payload.user_id);
        a.updated_at = Set(Some(crate::admin::data::now()));
        let is_default = a.is_default.clone().unwrap() == Some(true);
        a.update(&self.state.db).await.map_err(db_err)?;
        if is_default {
            self.clear_other_defaults(payload.tenant_id, req.id as i64)
                .await?;
        }
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteAiProviderRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::ai::service::v1::delete_ai_provider_request::QueryBy
        );
        let repo =
            crate::admin::data::repos::AiProviderRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        repo.find_scoped(id as i64)
            .await?
            .ok_or_else(|| not_found("ai provider"))?;
        crate::admin::data::sys_ai_providers::Entity::delete_by_id(id as i64)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_hint_masks_tail_and_keeps_short_keys() {
        // 长 key：尾 4 位
        assert_eq!(hint_of("sk-abcdefghijklmnop"), "sk-***mnop");
        // 短 key（≤8）：全量展示（没有可遮的尾）
        assert_eq!(hint_of("abc12345"), "sk-***abc12345");
        // 空白归一
        assert_eq!(hint_of("  xyz  "), "sk-***xyz");
        assert_eq!(hint_of(""), "");
        assert_eq!(hint_of("   "), "");
    }

    #[test]
    fn model_type_maps_round_trip_across_the_enum() {
        // 全部 8 个枚举值双向 round-trip；未知值 None 落 0
        for (i, name) in [
            (1, "OPENAI"),
            (2, "AZURE_OPENAI"),
            (3, "OLLAMA"),
            (4, "OPENAI_COMPATIBLE"),
            (5, "MOONSHOT"),
            (6, "ZHIPU_AI"),
            (7, "QWEN"),
            (8, "DEEPSEEK"),
        ] {
            assert_eq!(model_type_str(i), Some(name), "proto {i}");
            assert_eq!(model_type_proto(name), i, "ent {name}");
        }
        assert_eq!(model_type_str(0), None);
        assert_eq!(model_type_str(99), None);
        assert_eq!(model_type_proto("UNKNOWN"), 0);
    }

    // ---- update_mask 门控（sqlite 内存库走真 handler） ----

    use crate::data::sys_ai_providers as providers;
    use crate::testsupport::{ctx, test_state};
    use proto::gen::services::AiProviderServiceHandlers as _;
    use sea_orm::EntityTrait;

    async fn plant(db: &sea_orm::DatabaseConnection) -> providers::Model {
        providers::ActiveModel {
            tenant_id: Set(Some(1)),
            name: Set(Some("deepseek".into())),
            model_type: Set(Some("OPENAI".into())),
            base_url: Set(Some("https://api.deepseek.com/v1".into())),
            remark: Set(Some("旧备注".into())),
            is_enabled: Set(Some(true)),
            api_key: Set(Some("enc:old-key".into())),
            api_key_hint: Set(Some("sk-***old".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> providers::Model {
        providers::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn update_req(id: u64, data: AiProvider, paths: &[&str]) -> UpdateAiProviderRequest {
        UpdateAiProviderRequest {
            id,
            data: Some(data),
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
            allow_missing: None,
        }
    }

    /// update_mask 白名单门（字段级更新语义）：掩码只选 name 时，载荷里
    /// 带的 remark/is_enabled/model_type 都不许动——开放 API 按契约送
    /// 部分字段不再误写开关与枚举。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = AiProviderService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db).await;

        svc.update(
            me,
            update_req(
                row.id as u64,
                AiProvider {
                    name: Some("新名".into()),
                    remark: Some("劫持".into()),
                    is_enabled: Some(false),
                    model_type: Some(3), // OLLAMA——未选中，不得落列
                    ..Default::default()
                },
                &["name"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.name.as_deref(), Some("新名"), "masked field lands");
        assert_eq!(
            fresh.remark.as_deref(),
            Some("旧备注"),
            "unmasked remark is not writable through the gate"
        );
        assert_eq!(
            fresh.is_enabled,
            Some(true),
            "unmasked is_enabled is not writable through the gate"
        );
        assert_eq!(
            fresh.model_type.as_deref(),
            Some("OPENAI"),
            "unmasked model_type is not writable through the gate"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = AiProviderService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db).await;

        // 无掩码：载荷 None 不动列。
        svc.update(
            ctx(0, 1),
            UpdateAiProviderRequest {
                id: row.id as u64,
                data: Some(AiProvider {
                    name: Some("改名".into()),
                    ..Default::default()
                }),
                update_mask: None,
                allow_missing: None,
            },
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.remark.as_deref(),
            Some("旧备注"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update(
            me,
            update_req(row.id as u64, AiProvider::default(), &["remark"]),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.remark.is_none(),
            "masked None explicitly clears the column"
        );
    }

    /// api_key 写面同样过门：掩码未选中时载荷带新密钥也不落库（明文与
    /// hint 原样）；掩码选中且非空才加密落库并刷新 hint（测试态无加密
    /// 配置，encrypt_if_needed 明文直通）。
    #[tokio::test]
    async fn api_key_write_respects_the_mask() {
        let state = test_state().await;
        let svc = AiProviderService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db).await;

        // 未选中：新密钥不落库。
        svc.update(
            ctx(0, 1),
            update_req(
                row.id as u64,
                AiProvider {
                    api_key: Some("sk-new-secret".into()),
                    ..Default::default()
                },
                &["name"],
            ),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.api_key.as_deref(),
            Some("enc:old-key"),
            "unmasked api_key never lands"
        );
        assert_eq!(fresh.api_key_hint.as_deref(), Some("sk-***old"));

        // 选中且非空：加密落库 + hint 刷新。
        svc.update(
            me,
            update_req(
                row.id as u64,
                AiProvider {
                    api_key: Some("sk-new-secret".into()),
                    ..Default::default()
                },
                &["api_key"],
            ),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.api_key.as_deref(),
            Some("sk-new-secret"),
            "masked non-empty key lands (plaintext without the encryptor)"
        );
        assert_eq!(
            fresh.api_key_hint.as_deref(),
            Some("sk-***cret"),
            "hint refreshes to the new tail"
        );
    }
}
