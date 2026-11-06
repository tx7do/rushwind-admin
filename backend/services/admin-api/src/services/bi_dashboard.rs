//! BiDashboardService — 管理设计器产出的仪表盘 / 大屏 / 模板。
//! schema 存 payload 列（JSON 原文），服务端不解析只透传——与打印模板同口径。
//! Update 带乐观锁：data.version 回传读到的版本，CAS 命中才写入且版本 +1；
//! 未带 version 的轻量更新照常写、版本照样 +1（与上游 repo 同语义）。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, EntityTrait, Set};

use crate::data::Viewer;
use crate::state::{db_err, operator_of, require_data, status_error, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::bi::service::v1::{
    BiDashboard, CopyBiDashboardRequest, CreateBiDashboardRequest, DeleteBiDashboardRequest,
    GetBiDashboardRequest, ListBiDashboardResponse, UpdateBiDashboardRequest,
};
use proto::proto::pagination::PagingRequest;

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

fn dash_proto(r: crate::data::bi_dashboards::Model) -> BiDashboard {
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
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

fn upsert_fields(a: &mut crate::data::bi_dashboards::ActiveModel, data: &BiDashboard) {
    if let Some(v) = &data.name {
        a.name = Set(v.clone());
    }
    if let Some(v) = &data.code {
        a.code = Set(v.clone());
    }
    if let Some(v) = data.kind {
        a.kind = Set(Some(kind_str(v).into()));
    }
    if let Some(v) = &data.remark {
        a.remark = Set(Some(v.clone()));
    }
    if let Some(v) = &data.payload {
        a.payload = Set(Some(v.clone()));
    }
    if let Some(v) = data.chart_count {
        a.chart_count = Set(Some(v));
    }
    if let Some(v) = &data.theme_key {
        a.theme_key = Set(Some(v.clone()));
    }
    if let Some(v) = &data.thumbnail {
        a.thumbnail = Set(Some(v.clone()));
    }
    if let Some(v) = data.is_built_in {
        a.is_built_in = Set(Some(v));
    }
    if let Some(v) = data.is_public {
        a.is_public = Set(Some(v));
    }
    if let Some(v) = data.is_enabled {
        a.is_enabled = Set(Some(v));
    }
    if let Some(v) = data.sort_order {
        a.sort_order = Set(Some(v));
    }
}

pub struct BiDashboardService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::BiDashboardServiceHandlers for BiDashboardService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListBiDashboardResponse, StatusError> {
        let repo = crate::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListBiDashboardResponse {
            items: rows.into_iter().map(dash_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetBiDashboardRequest,
    ) -> Result<BiDashboard, StatusError> {
        let repo = crate::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = match req.query_by {
            Some(proto::proto::bi::service::v1::get_bi_dashboard_request::QueryBy::Id(id)) => {
                repo.find_scoped(id).await?
            }
            None => None,
        }
        .ok_or_else(|| crate::state::not_found("bi dashboard"))?;
        Ok(dash_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateBiDashboardRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo = crate::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        if let Some(code) = data.code.as_deref().filter(|c| !c.is_empty()) {
            if repo.find_by_code(code).await?.is_some() {
                return Err(status_error("CONFLICT", "dashboard code already exists"));
            }
        }
        let mut a = crate::data::bi_dashboards::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            name: Set(data.name.clone().unwrap_or_default()),
            code: Set(data.code.clone().unwrap_or_default()),
            kind: Set(Some(kind_str(data.kind.unwrap_or(0)).into())),
            version: Set(Some(1)),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        };
        upsert_fields(&mut a, &data);
        if a.is_built_in.clone().unwrap().is_none() {
            a.is_built_in = Set(Some(false));
        }
        if a.is_enabled.clone().unwrap().is_none() {
            a.is_enabled = Set(Some(true));
        }
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
        let repo = crate::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = repo
            .find_scoped(req.id)
            .await?
            .ok_or_else(|| crate::state::not_found("bi dashboard"))?;
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
        let mut a: crate::data::bi_dashboards::ActiveModel = row.into();
        upsert_fields(&mut a, &data);
        crate::stamp_update!(a, payload.user_id);
        a.version = Set(Some(a.version.clone().unwrap().unwrap_or(0) + 1));
        a.updated_at = Set(Some(crate::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteBiDashboardRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::bi::service::v1::delete_bi_dashboard_request::QueryBy
        );
        let repo = crate::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = repo
            .find_scoped(id)
            .await?
            .ok_or_else(|| crate::state::not_found("bi dashboard"))?;
        // 内置行禁删（内置模板只读，派生出来的才是租户自有资产）。
        if row.is_built_in == Some(true) {
            return Err(status_error(
                "FORBIDDEN",
                "built-in dashboard cannot be deleted",
            ));
        }
        crate::data::bi_dashboards::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
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
        let repo = crate::data::repos::BiDashboardRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let source = repo
            .find_scoped(req.id)
            .await?
            .ok_or_else(|| crate::state::not_found("bi dashboard"))?;

        let name = match req.name.filter(|s| !s.is_empty()) {
            Some(v) => v,
            None => format!("{}-copy", source.name),
        };
        let code = match req.code.filter(|s| !s.is_empty()) {
            Some(v) => v,
            None => {
                let suffix: String = crate::token::new_jwt_id().chars().take(8).collect();
                format!("{}-copy-{}", source.code, suffix)
            }
        };
        if repo.find_by_code(&code).await?.is_some() {
            return Err(status_error("CONFLICT", "dashboard code already exists"));
        }

        crate::data::bi_dashboards::ActiveModel {
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
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
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
    fn upsert_fields_only_touches_payload_fields() {
        // version 与 operator/时间戳列永不进 upsert_fields（乐观锁与盖章
        // 是 handler 的职责，payload 字段单独走）——钉住这条边界。
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
            version: Some(7), // 不得出现在 upsert_fields 里
            is_enabled: None,
            sort_order: Some(2),
            created_by: None,
            updated_by: None,
            deleted_by: None,
            created_at: None,
            updated_at: None,
            deleted_at: None,
        };
        let mut a = crate::data::bi_dashboards::ActiveModel {
            version: Set(Some(1)),
            ..Default::default()
        };
        upsert_fields(&mut a, &data);
        assert_eq!(a.name.clone().unwrap(), "n");
        assert_eq!(a.kind.clone().unwrap(), Some("SCREEN".to_string()));
        assert_eq!(a.chart_count.clone().unwrap(), Some(3));
        assert_eq!(a.is_public.clone().unwrap(), Some(true));
        assert_eq!(a.sort_order.clone().unwrap(), Some(2));
        // 版本未被 upsert 触碰（仍是 handler 预置的 1）
        assert_eq!(a.version.clone().unwrap(), Some(1));
        assert!(matches!(a.is_enabled, sea_orm::ActiveValue::NotSet));
    }
}
