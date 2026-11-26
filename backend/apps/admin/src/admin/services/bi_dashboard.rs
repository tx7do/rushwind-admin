//! BiDashboardService — 管理设计器产出的仪表盘 / 大屏 / 模板。
//! schema 存 payload 列（JSON 原文），服务端不解析只透传——与打印模板同口径。
//! Update 带乐观锁：data.version 回传读到的版本，CAS 命中才写入且版本 +1；
//! 未带 version 的轻量更新照常写、版本照样 +1（与上游 repo 同语义）。
//! 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即存量
//! 全量提交语义）——乐观锁与版本快照的特殊逻辑原位保留。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, EntityTrait, Set};

use crate::admin::data::Viewer;
use crate::admin::state::{db_err, operator_of, require_data, status_error, AppState, StatusError};
use crate::proto::bi::service::v1::{
    BiDashboard, BiDashboardRevision, CopyBiDashboardRequest, CreateBiDashboardRequest,
    DeleteBiDashboardRequest, GetBiDashboardRequest, ListBiDashboardResponse,
    ListBiDashboardRevisionsRequest, ListBiDashboardRevisionsResponse,
    RestoreBiDashboardRevisionRequest, UpdateBiDashboardRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

fn kind_str(v: i32) -> &'static str {
    match v {
        1 => "SCREEN",
        2 => "TEMPLATE",
        _ => "DASHBOARD",
    }
}

fn kind_proto(s: &str) -> i32 {
    match s {
        "SCREEN" => 1,
        "TEMPLATE" => 2,
        _ => 0,
    }
}

fn dash_proto(r: crate::admin::data::bi_dashboards::Model) -> BiDashboard {
    BiDashboard {
        id: Some(r.id),
        name: Some(r.name),
        code: Some(r.code),
        kind: r.kind.as_deref().map(kind_proto),
        remark: r.remark,
        payload: r.payload,
        chart_count: r.chart_count,
        theme_key: r.theme_key,
        thumbnail: r.thumbnail,
        is_built_in: r.is_built_in,
        is_public: r.is_public,
        version: r.version,
        is_enabled: r.is_enabled,
        sort_order: r.sort_order,
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::admin::state::naive_to_ts),
    }
}

fn rev_proto(r: crate::admin::data::bi_dashboard_revisions::Model) -> BiDashboardRevision {
    BiDashboardRevision {
        id: r.id,
        dashboard_id: r.dashboard_id.unwrap_or(0),
        version: r.version.unwrap_or(0),
        name: r.name,
        payload: r.payload,
        source: r.source,
        created_by: r.created_by,
        // 与上游 DTO 同口径："2006-01-02 15:04:05"
        created_at: r
            .created_at
            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()),
    }
}

/// 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即存量
/// if-let-Some 全量提交语义：None 不动列）。payload 是设计器产出的 JSON
/// 原文——服务端不解析只透传，layout 部分更新无从谈起，保持整列写；
/// version 是乐观锁，永不进本函数（CAS 与 +1 是 handler 的职责）。
fn apply_fields(
    a: &mut crate::data::bi_dashboards::ActiveModel,
    data: &BiDashboard,
    mask: &crate::fieldmask::FieldMask,
) {
    crate::set_masked!(req mask, a, name, data.name, |v: &String| v.clone());
    crate::set_masked!(req mask, a, code, data.code, |v: &String| v.clone());
    crate::set_masked!(opt mask, a, kind, data.kind, |v: &i32| Some(
        kind_str(*v).to_string()
    ));
    crate::set_masked!(opt mask, a, remark, data.remark, |v: &String| Some(v.clone()));
    crate::set_masked!(opt mask, a, payload, data.payload, |v: &String| Some(
        v.clone()
    ));
    crate::set_masked!(opt mask, a, chart_count, data.chart_count, |v: &u32| Some(*v));
    crate::set_masked!(opt mask, a, theme_key, data.theme_key, |v: &String| Some(
        v.clone()
    ));
    crate::set_masked!(opt mask, a, thumbnail, data.thumbnail, |v: &String| Some(
        v.clone()
    ));
    crate::set_masked!(opt mask, a, is_built_in, data.is_built_in, |v: &bool| Some(
        *v
    ));
    crate::set_masked!(opt mask, a, is_public, data.is_public, |v: &bool| Some(*v));
    crate::set_masked!(opt mask, a, is_enabled, data.is_enabled, |v: &bool| Some(*v));
    crate::set_masked!(opt mask, a, sort_order, data.sort_order, |v: &u32| Some(*v));
}

pub struct BiDashboardService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::services::BiDashboardServiceHandlers for BiDashboardService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListBiDashboardResponse, StatusError> {
        let repo =
            crate::admin::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListBiDashboardResponse {
            items: rows.into_iter().map(dash_proto).collect(),
            total: total as i64,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetBiDashboardRequest,
    ) -> Result<BiDashboard, StatusError> {
        let repo =
            crate::admin::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = match req.query_by {
            Some(crate::proto::bi::service::v1::get_bi_dashboard_request::QueryBy::Id(id)) => {
                repo.find_scoped(id).await?
            }
            None => None,
        }
        .ok_or_else(|| crate::admin::state::not_found("bi dashboard"))?;
        Ok(dash_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateBiDashboardRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo =
            crate::admin::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        if let Some(code) = data.code.as_deref().filter(|c| !c.is_empty()) {
            if repo.find_by_code(code).await?.is_some() {
                return Err(status_error("CONFLICT", "dashboard code already exists"));
            }
        }
        let mut a = crate::admin::data::bi_dashboards::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            name: Set(data.name.clone().unwrap_or_default()),
            code: Set(data.code.clone().unwrap_or_default()),
            kind: Set(Some(kind_str(data.kind.unwrap_or(0)).into())),
            version: Set(Some(1)),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        };
        apply_fields(&mut a, &data, &crate::fieldmask::FieldMask::unmasked());
        crate::ensure_default!(a, is_built_in, false);
        crate::ensure_default!(a, is_enabled, true);
        a.insert(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateBiDashboardRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo =
            crate::admin::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = repo
            .find_scoped(req.id)
            .await?
            .ok_or_else(|| crate::admin::state::not_found("bi dashboard"))?;
        if let Some(code) = data.code.as_deref().filter(|c| !c.is_empty()) {
            if let Some(other) = repo.find_by_code(code).await? {
                if other.id != req.id {
                    return Err(status_error("CONFLICT", "dashboard code already exists"));
                }
            }
        }
        // 乐观锁：data.version 回传读到的版本时按 CAS 校验，冲突 BadRequest；
        // 版本号无论走哪条路都 +1。
        if let Some(want) = data.version {
            if row.version.unwrap_or(0) != want {
                return Err(status_error(
                    "BAD_REQUEST",
                    format!(
                        "dashboard version conflict: expected {}, got {}",
                        row.version.unwrap_or(0),
                        want
                    ),
                ));
            }
        }
        // 版本快照（docs/bi_dashboard.md 后续项）：保存成功把**保存前**的
        // payload/name/version 留一行。快照失败不阻断保存（留痕）——
        // 主功能不应被辅功能拖死，但丢快照必须可观测。
        let pre = (
            row.tenant_id,
            row.version,
            row.name.clone(),
            row.payload.clone(),
        );
        let mut a: crate::data::bi_dashboards::ActiveModel = row.into();
        // 字段级更新：掩码白名单外字段不动（无掩码 = 全量提交语义不变）；
        // 乐观锁 CAS 与版本 +1 是本 handler 的职责，不受掩码影响。
        let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
        apply_fields(&mut a, &data, &mask);
        crate::stamp_update!(a, payload.user_id);
        a.version = Set(Some(a.version.clone().unwrap().unwrap_or(0) + 1));
        a.updated_at = Set(Some(crate::admin::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;

        let revisions = crate::admin::data::repos::BiDashboardRevisionRepo::new(
            &self.state.db,
            Viewer::from_ctx(&ctx),
        );
        if let Err(serr) = revisions
            .create(
                pre.0,
                req.id,
                pre.1,
                Some(pre.2),
                pre.3,
                "SAVE",
                payload.user_id,
            )
            .await
        {
            tracing::warn!(
                target: "rushwind.admin.bi_revision",
                dashboard_id = req.id,
                error = ?serr,
                "snapshot before update dashboard failed"
            );
        }
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteBiDashboardRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            crate::proto::bi::service::v1::delete_bi_dashboard_request::QueryBy
        );
        let repo =
            crate::admin::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = repo
            .find_scoped(id)
            .await?
            .ok_or_else(|| crate::admin::state::not_found("bi dashboard"))?;
        // 内置行禁删（内置模板只读，派生出来的才是租户自有资产）。
        if row.is_built_in == Some(true) {
            return Err(status_error(
                "FORBIDDEN",
                "built-in dashboard cannot be deleted",
            ));
        }
        crate::admin::data::bi_dashboards::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        // 快照随主表级联清理（SeaORM 无库级 cascade，显式随行）
        crate::admin::data::repos::BiDashboardRevisionRepo::new(
            &self.state.db,
            Viewer::from_ctx(&ctx),
        )
        .delete_for_dashboard(id)
        .await?;
        Ok(Empty {})
    }

    /// 复制（模板派生 / 另存为）：整份复制 payload 成一行新记录。
    /// 副本恒为非内置、版本归 1、启用；名称/编码缺省自动派生，编码需租户内
    /// 唯一——短随机后缀保证唯一（时间戳秒级在"同一秒复制两次"必撞）。
    async fn copy(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CopyBiDashboardRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let repo =
            crate::admin::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let source = repo
            .find_scoped(req.id)
            .await?
            .ok_or_else(|| crate::admin::state::not_found("bi dashboard"))?;

        let name = match req.name.filter(|s| !s.is_empty()) {
            Some(v) => v,
            None => format!("{}-copy", source.name),
        };
        let code = match req.code.filter(|s| !s.is_empty()) {
            Some(v) => v,
            None => {
                let suffix: String = crate::admin::token::new_jwt_id().chars().take(8).collect();
                format!("{}-copy-{}", source.code, suffix)
            }
        };
        if repo.find_by_code(&code).await?.is_some() {
            return Err(status_error("CONFLICT", "dashboard code already exists"));
        }

        crate::admin::data::bi_dashboards::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            name: Set(name),
            code: Set(code),
            kind: Set(source.kind.clone()),
            remark: Set(source.remark.clone()),
            payload: Set(source.payload.clone()),
            chart_count: Set(source.chart_count),
            theme_key: Set(source.theme_key.clone()),
            thumbnail: Set(source.thumbnail.clone()),
            // 副本恒非内置：内置模板只读，派生出来的才是租户自有资产
            is_built_in: Set(Some(false)),
            is_public: Set(source.is_public),
            version: Set(Some(1)),
            is_enabled: Set(Some(true)),
            sort_order: Set(source.sort_order),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        Ok(Empty {})
    }

    /// 版本历史（新快照在前）。
    async fn list_revisions(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: ListBiDashboardRevisionsRequest,
    ) -> Result<ListBiDashboardRevisionsResponse, StatusError> {
        if req.dashboard_id == 0 {
            return Err(status_error("BAD_REQUEST", "dashboardId is required"));
        }
        let repo = crate::admin::data::repos::BiDashboardRevisionRepo::new(
            &self.state.db,
            Viewer::from_ctx(&ctx),
        );
        let (rows, total) = repo
            .list_paged(
                req.dashboard_id,
                req.page.as_ref().and_then(|p| p.page),
                req.page.as_ref().and_then(|p| p.page_size),
            )
            .await?;
        Ok(ListBiDashboardRevisionsResponse {
            items: rows.into_iter().map(rev_proto).collect(),
            total: total as u32,
        })
    }

    /// 恢复版本：快照 payload/name 写回主表，版本号 +1。
    /// 恢复动作本身先把**当前版本**快照（RESTORE 来源），恢复可逆。
    async fn restore_revision(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: RestoreBiDashboardRevisionRequest,
    ) -> Result<Empty, StatusError> {
        if req.id == 0 {
            return Err(status_error("BAD_REQUEST", "id is required"));
        }
        let payload = operator_of(&ctx)?;
        let revisions = crate::admin::data::repos::BiDashboardRevisionRepo::new(
            &self.state.db,
            Viewer::from_ctx(&ctx),
        );
        let revision = revisions
            .find_scoped(req.id)
            .await?
            .ok_or_else(|| crate::admin::state::not_found("bi dashboard revision"))?;
        let dashboard_id = revision.dashboard_id.unwrap_or(0);
        if dashboard_id == 0 {
            return Err(crate::admin::state::not_found("bi dashboard"));
        }

        let repo =
            crate::admin::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let current = repo
            .find_scoped(dashboard_id)
            .await?
            .ok_or_else(|| crate::admin::state::not_found("bi dashboard"))?;

        // 恢复前留底当前版本（失败不阻断恢复，留痕可观测）
        if let Err(serr) = revisions
            .create(
                current.tenant_id,
                dashboard_id,
                current.version,
                Some(current.name.clone()),
                current.payload.clone(),
                "RESTORE",
                payload.user_id,
            )
            .await
        {
            tracing::warn!(
                target: "rushwind.admin.bi_revision",
                dashboard_id,
                error = ?serr,
                "snapshot before restore dashboard failed"
            );
        }

        // 写回走普通更新（无 CAS——恢复是明确的人为决定，以快照为准整份覆盖），
        // 版本号 +1（与保存同轨）。
        let mut a: crate::admin::data::bi_dashboards::ActiveModel = current.into();
        a.name = Set(revision.name.clone().unwrap_or_default());
        a.payload = Set(revision.payload.clone());
        crate::stamp_update!(a, payload.user_id);
        a.version = Set(Some(a.version.clone().unwrap().unwrap_or(0) + 1));
        a.updated_at = Set(Some(crate::admin::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_round_trip_with_dashboard_default() {
        // 三种显式值双向；未知值两侧都落 DASHBOARD/0
        for (i, name) in [(0, "DASHBOARD"), (1, "SCREEN"), (2, "TEMPLATE")] {
            assert_eq!(kind_str(i), name);
            assert_eq!(kind_proto(name), i);
        }
        // proto 未知码 → DASHBOARD；库中未知串 → 0
        assert_eq!(kind_str(99), "DASHBOARD");
        assert_eq!(kind_proto("GARBAGE"), 0);
    }

    #[test]
    fn apply_fields_only_touches_payload_fields() {
        // version 与 operator/时间戳列永不进 apply_fields（乐观锁与盖章
        // 是 handler 的职责，payload 字段单独走）——钉住这条边界。
        // 无掩码（unmasked）= 存量全量提交语义，逐字段行为与旧 if-let 一致。
        let data = BiDashboard {
            id: None,
            name: Some("n".into()),
            code: Some("c".into()),
            kind: Some(1),
            remark: None,
            payload: Some("{}".into()),
            chart_count: Some(3),
            theme_key: None,
            thumbnail: None,
            is_built_in: None,
            is_public: Some(true),
            version: Some(7), // 不得出现在 apply_fields 里
            is_enabled: None,
            sort_order: Some(2),
            created_by: None,
            updated_by: None,
            deleted_by: None,
            created_at: None,
            updated_at: None,
            deleted_at: None,
        };
        let mut a = crate::admin::data::bi_dashboards::ActiveModel {
            version: Set(Some(1)),
            ..Default::default()
        };
        apply_fields(&mut a, &data, &crate::fieldmask::FieldMask::from(None));
        assert_eq!(a.name.clone().unwrap(), "n");
        assert_eq!(a.kind.clone().unwrap(), Some("SCREEN".to_string()));
        assert_eq!(a.chart_count.clone().unwrap(), Some(3));
        assert_eq!(a.is_public.clone().unwrap(), Some(true));
        assert_eq!(a.sort_order.clone().unwrap(), Some(2));
        // 版本未被 upsert 触碰（仍是 handler 预置的 1）
        assert_eq!(a.version.clone().unwrap(), Some(1));
        assert!(matches!(a.is_enabled, sea_orm::ActiveValue::NotSet));
    }

    #[test]
    fn rev_proto_maps_snapshot_fields_with_timestamp_format() {
        let rev = crate::admin::data::bi_dashboard_revisions::Model {
            id: 7,
            tenant_id: Some(3),
            dashboard_id: Some(42),
            version: Some(5),
            name: Some("before-save".into()),
            payload: Some(r#"{"charts":[]}"#.into()),
            source: Some("SAVE".into()),
            created_by: Some(11),
            created_at: Some(
                chrono::NaiveDate::from_ymd_opt(2026, 11, 8)
                    .unwrap()
                    .and_hms_opt(9, 30, 5)
                    .unwrap(),
            ),
            updated_at: None,
        };
        let proto = rev_proto(rev);
        assert_eq!(proto.id, 7);
        assert_eq!(proto.dashboard_id, 42);
        assert_eq!(proto.version, 5);
        assert_eq!(proto.name.as_deref(), Some("before-save"));
        assert_eq!(proto.source.as_deref(), Some("SAVE"));
        assert_eq!(proto.created_by, Some(11));
        // 与上游 DTO 同口径的时间串
        assert_eq!(proto.created_at.as_deref(), Some("2026-11-08 09:30:05"));

        // 缺省口径：version/dashboard_id 空值落 0，created_at 空不回填
        let bare = crate::admin::data::bi_dashboard_revisions::Model {
            id: 8,
            tenant_id: None,
            dashboard_id: None,
            version: None,
            name: None,
            payload: None,
            source: None,
            created_by: None,
            created_at: None,
            updated_at: None,
        };
        let proto = rev_proto(bare);
        assert_eq!(proto.dashboard_id, 0);
        assert_eq!(proto.version, 0);
        assert!(proto.name.is_none());
        assert!(proto.created_at.is_none());
    }
}

#[cfg(test)]
mod update_mask_tests {
    use super::*;
    use crate::data::bi_dashboards as dashboards;
    use crate::proto::gen::services::BiDashboardServiceHandlers as _;
    use crate::testsupport::{ctx, test_state};
    use sea_orm::EntityTrait;

    async fn plant(db: &sea_orm::DatabaseConnection, name: &str, code: &str) -> dashboards::Model {
        dashboards::ActiveModel {
            tenant_id: Set(Some(1)),
            name: Set(name.into()),
            code: Set(code.into()),
            kind: Set(Some("DASHBOARD".into())),
            remark: Set(Some("旧备注".into())),
            payload: Set(Some(r#"{"charts":[]}"#.into())),
            theme_key: Set(Some("dark".into())),
            version: Set(Some(1)),
            is_built_in: Set(Some(false)),
            is_enabled: Set(Some(true)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> dashboards::Model {
        dashboards::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn update_req(id: i64, data: BiDashboard, paths: &[&str]) -> UpdateBiDashboardRequest {
        UpdateBiDashboardRequest {
            id,
            data: Some(data),
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
            allow_missing: None,
        }
    }

    /// update_mask 白名单门（字段级更新语义）：掩码只选 name 时，载荷里
    /// 带的 remark/payload 都不许动——设计器产出的 schema 原文不被部分
    /// 更新的表单抹掉；版本号照常 +1（乐观锁不受掩码影响）。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = BiDashboardService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "运营大盘", "dash-1").await;

        svc.update(
            me,
            update_req(
                row.id,
                BiDashboard {
                    name: Some("新名".into()),
                    remark: Some("劫持".into()),
                    payload: Some(r#"{"evil":true}"#.into()),
                    ..Default::default()
                },
                &["name"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.name, "新名", "masked field lands");
        assert_eq!(
            fresh.remark.as_deref(),
            Some("旧备注"),
            "unmasked remark is not writable through the gate"
        );
        assert_eq!(
            fresh.payload.as_deref(),
            Some(r#"{"charts":[]}"#),
            "unmasked payload (schema json) is not writable through the gate"
        );
        assert_eq!(
            fresh.version,
            Some(2),
            "version still bumps +1 (optimistic lock is mask-agnostic)"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = BiDashboardService {
            state: state.clone(),
        };
        let me = ctx(0, 1);
        let db = &state.db;
        let row = plant(db, "运营大盘", "dash-1").await;

        // 无掩码：载荷 None 不动列。
        svc.update(
            ctx(0, 1),
            UpdateBiDashboardRequest {
                id: row.id,
                data: Some(BiDashboard {
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
        svc.update(me, update_req(row.id, BiDashboard::default(), &["remark"]))
            .await
            .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.remark.is_none(),
            "masked None explicitly clears the column"
        );
    }
}
