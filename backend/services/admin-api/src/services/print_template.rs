//! PrintTemplateService — 打印模板管理 + 服务端 PDF 渲染。
//! schema 存 payload 列（JSON 原文），服务端不解析只透传——与 BI 仪表盘同口径。
//! RenderPdf 接前端内核产出的**最终文档 HTML**（分页/毫米布局全在前端完成），
//! 本服务只做 HTML → PDF 一步（见 [`super::printrender`]）——后端不重排、
//! 不理解模板，纸张宽高与方向由调用方按模板元数据传入，保证"预览即所得"。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, EntityTrait, Set};

use crate::data::Viewer;
use crate::state::{
    db_err, not_found, operator_of, require_data, status_error, AppState, StatusError,
};
use pbjson_types::Empty;
use proto::proto::pagination::PagingRequest;
use proto::proto::print::service::v1::{
    CreatePrintTemplateRequest, DeletePrintTemplateRequest, GetPrintTemplateRequest,
    ListPrintTemplateResponse, PrintTemplate, RenderPrintTemplateRequest,
    RenderPrintTemplateResponse, UpdatePrintTemplateRequest,
};

fn tpl_proto(r: crate::data::print_templates::Model) -> PrintTemplate {
    PrintTemplate {
        id: Some(r.id),
        name: Some(r.name),
        code: Some(r.code),
        resource_key: r.resource_key,
        payload: r.payload,
        is_built_in: r.is_built_in,
        paper_size: r.paper_size,
        orientation: r.orientation,
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

fn upsert_fields(a: &mut crate::data::print_templates::ActiveModel, data: &PrintTemplate) {
    if let Some(v) = &data.name {
        a.name = Set(v.clone());
    }
    if let Some(v) = &data.code {
        a.code = Set(v.clone());
    }
    if let Some(v) = &data.resource_key {
        a.resource_key = Set(Some(v.clone()));
    }
    if let Some(v) = &data.payload {
        a.payload = Set(Some(v.clone()));
    }
    if let Some(v) = data.is_built_in {
        a.is_built_in = Set(Some(v));
    }
    if let Some(v) = &data.paper_size {
        a.paper_size = Set(Some(v.clone()));
    }
    if let Some(v) = &data.orientation {
        a.orientation = Set(Some(v.clone()));
    }
    if let Some(v) = data.is_enabled {
        a.is_enabled = Set(Some(v));
    }
    if let Some(v) = data.sort_order {
        a.sort_order = Set(Some(v));
    }
}

pub struct PrintTemplateService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::PrintTemplateServiceHandlers for PrintTemplateService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListPrintTemplateResponse, StatusError> {
        let repo =
            crate::data::repos::PrintTemplateRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListPrintTemplateResponse {
            items: rows.into_iter().map(tpl_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetPrintTemplateRequest,
    ) -> Result<PrintTemplate, StatusError> {
        let repo =
            crate::data::repos::PrintTemplateRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = match req.query_by {
            Some(proto::proto::print::service::v1::get_print_template_request::QueryBy::Id(id)) => {
                repo.find_scoped(id).await?
            }
            None => None,
        }
        .ok_or_else(|| not_found("print template"))?;
        Ok(tpl_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreatePrintTemplateRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo =
            crate::data::repos::PrintTemplateRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        if let Some(code) = data.code.as_deref().filter(|c| !c.is_empty()) {
            if repo.find_by_code(code).await?.is_some() {
                return Err(status_error("CONFLICT", "template code already exists"));
            }
        }
        let mut a = crate::data::print_templates::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            name: Set(data.name.clone().unwrap_or_default()),
            code: Set(data.code.clone().unwrap_or_default()),
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
        req: UpdatePrintTemplateRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo =
            crate::data::repos::PrintTemplateRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = repo
            .find_scoped(req.id)
            .await?
            .ok_or_else(|| not_found("print template"))?;
        if let Some(code) = data.code.as_deref().filter(|c| !c.is_empty()) {
            if let Some(other) = repo.find_by_code(code).await? {
                if other.id != req.id {
                    return Err(status_error("CONFLICT", "template code already exists"));
                }
            }
        }
        let mut a: crate::data::print_templates::ActiveModel = row.into();
        upsert_fields(&mut a, &data);
        crate::stamp_update!(a, payload.user_id);
        a.updated_at = Set(Some(crate::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeletePrintTemplateRequest,
    ) -> Result<Empty, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::print::service::v1::delete_print_template_request::QueryBy
        );
        let repo =
            crate::data::repos::PrintTemplateRepo::new(&self.state.db, Viewer::from_ctx(&ctx));
        let row = repo
            .find_scoped(id)
            .await?
            .ok_or_else(|| not_found("print template"))?;
        // 内置行禁删（与上游 repo 同则）。
        if row.is_built_in == Some(true) {
            return Err(status_error(
                "BAD_REQUEST",
                "built-in print template cannot be deleted",
            ));
        }
        crate::data::print_templates::Entity::delete_by_id(id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }

    /// 服务端渲染：前端内核产出的最终 HTML → 本机 Chrome 打印成 PDF。
    /// 纸张宽高（毫米）与方向由调用方按模板元数据传入，保证"预览即所得"。
    async fn render_pdf(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: RenderPrintTemplateRequest,
    ) -> Result<RenderPrintTemplateResponse, StatusError> {
        let pdf = super::printrender::render_pdf(super::printrender::PrintOptions {
            html: req.html,
            paper_width_mm: req.paper_width_mm,
            paper_height_mm: req.paper_height_mm,
            landscape: req.landscape,
            background: req.background,
        })
        .await
        .map_err(|e| status_error("INTERNAL_SERVER_ERROR", e))?;
        let filename = match req.filename.is_empty() {
            true => "print.pdf".to_string(),
            false => format!("{}.pdf", req.filename),
        };
        Ok(RenderPrintTemplateResponse { pdf, filename })
    }
}
