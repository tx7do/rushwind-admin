//! NotificationTemplateService — 通知模板管理（平台全局）：CRUD + 试渲染。
//! 渲染内核 [`render_template`] 同时是发送链路 template_code 路径的复用点，
//! 两处共享同一套占位符语义。
//!
//! 占位符语义（与上游逐字一致）：
//!   - 未识别占位符 → 400 带占位符名：静默渲染空串会让"变量名拼错"漏到
//!     对端才现形，排查方向整个是反的；
//!   - `{{` 未闭合 → 按正文原样保留（模板正文里出现 JSON 片段是合法需求）；
//!   - 值不做任何转义：模板产物是纯文本（邮件正文/站内信正文），不是 JSON。

use std::collections::BTreeMap;
use std::sync::Arc;

use sea_orm::ActiveModelTrait;
use sea_orm::Set;

use crate::state::{
    db_err, not_found, operator_of, require_data, status_error, AppState, StatusError,
};
use pbjson_types::Empty;
use proto::proto::notification::service::v1::{
    CreateNotificationTemplateRequest, DeleteNotificationTemplateRequest,
    GetNotificationTemplateRequest, ListNotificationTemplateResponse, NotificationTemplate,
    RenderNotificationTemplateRequest, RenderNotificationTemplateResponse,
    UpdateNotificationTemplateRequest,
};
use proto::proto::pagination::PagingRequest;

fn template_proto(r: crate::data::sys_notification_templates::Model) -> NotificationTemplate {
    NotificationTemplate {
        id: Some(r.id),
        name: r.name,
        code: r.code,
        title_template: r.title_template,
        content_template: r.content_template,
        is_enabled: r.is_enabled,
        remark: r.remark,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
    }
}

fn upsert_fields(
    a: &mut crate::data::sys_notification_templates::ActiveModel,
    data: &NotificationTemplate,
) {
    if let Some(v) = &data.name {
        a.name = Set(Some(v.clone()));
    }
    if let Some(v) = &data.code {
        a.code = Set(Some(v.clone()));
    }
    if let Some(v) = &data.title_template {
        a.title_template = Set(Some(v.clone()));
    }
    if let Some(v) = &data.content_template {
        a.content_template = Set(Some(v.clone()));
    }
    if let Some(v) = data.is_enabled {
        a.is_enabled = Set(Some(v));
    }
    if let Some(v) = &data.remark {
        a.remark = Set(Some(v.clone()));
    }
}

pub struct NotificationTemplateService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::NotificationTemplateServiceHandlers for NotificationTemplateService {
    /// 模板列表（分页，contains 搜索）。
    async fn list_notification_template(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListNotificationTemplateResponse, StatusError> {
        let repo = crate::data::repos::NotificationTemplateRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListNotificationTemplateResponse {
            items: rows.into_iter().map(template_proto).collect(),
            total,
        })
    }

    /// 模板详情：id 优先，其次按 code（发送方的引用锚）。
    async fn get_notification_template(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetNotificationTemplateRequest,
    ) -> Result<NotificationTemplate, StatusError> {
        let repo = crate::data::repos::NotificationTemplateRepo::new(&self.state.db);
        let id = match &req.query_by {
            Some(proto::proto::notification::service::v1::get_notification_template_request::QueryBy::Id(id)) => *id,
            Some(proto::proto::notification::service::v1::get_notification_template_request::QueryBy::Code(code)) => {
                let row = repo
                    .get_by_code(code)
                    .await?
                    .ok_or_else(|| not_found("notification template"))?;
                return Ok(template_proto(row));
            }
            None => 0,
        };
        if id != 0 {
            return Ok(template_proto(repo.get_by_id(id).await?));
        }
        Err(status_error("BAD_REQUEST", "id or code is required"))
    }

    /// 新建模板。code 全局唯一（发送方引用锚），冲突 409。
    /// 不做"空变量集试渲染"：模板带占位符是它的本义，值在发送时才给。
    async fn create_notification_template(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateNotificationTemplateRequest,
    ) -> Result<NotificationTemplate, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo = crate::data::repos::NotificationTemplateRepo::new(&self.state.db);
        if let Some(code) = data.code.as_deref().filter(|c| !c.is_empty()) {
            if repo.get_by_code(code).await?.is_some() {
                return Err(status_error("CONFLICT", "template code already exists"));
            }
        }
        let mut a = crate::data::sys_notification_templates::ActiveModel {
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        };
        upsert_fields(&mut a, &data);
        if let sea_orm::ActiveValue::Set(v) = &a.is_enabled {
            if v.is_none() {
                a.is_enabled = Set(Some(true));
            }
        } else {
            a.is_enabled = Set(Some(true));
        }
        let inserted = a.insert(&self.state.db).await.map_err(db_err)?;
        Ok(template_proto(inserted))
    }

    /// 更新模板（请求体 {data:{...}}；update_mask 面向后端字段级更新，
    /// 本仓沿用 if-let-Some 全量提交语义）。
    async fn update_notification_template(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateNotificationTemplateRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo = crate::data::repos::NotificationTemplateRepo::new(&self.state.db);
        let row = repo.get_by_id(req.id).await?;
        if let Some(code) = data.code.as_deref().filter(|c| !c.is_empty()) {
            if let Some(other) = repo.get_by_code(code).await? {
                if other.id != req.id {
                    return Err(status_error("CONFLICT", "template code already exists"));
                }
            }
        }
        let mut a: crate::data::sys_notification_templates::ActiveModel = row.into();
        upsert_fields(&mut a, &data);
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    /// 删除模板。
    async fn delete_notification_template(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteNotificationTemplateRequest,
    ) -> Result<Empty, StatusError> {
        let repo = crate::data::repos::NotificationTemplateRepo::new(&self.state.db);
        repo.get_by_id(req.id).await?;
        repo.delete_by_id(req.id).await?;
        Ok(Empty {})
    }

    /// 试渲染：变量集给了什么就渲染什么，模板引用了变量集之外的占位符
    /// → 400 带占位符名（管理页预览用，不产生投递）。
    async fn render_notification_template(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: RenderNotificationTemplateRequest,
    ) -> Result<RenderNotificationTemplateResponse, StatusError> {
        let repo = crate::data::repos::NotificationTemplateRepo::new(&self.state.db);
        let tpl = repo.get_by_id(req.id).await?;
        let vars: BTreeMap<String, String> = req.variables.into_iter().collect();

        let title = render_template(&tpl.title_template.unwrap_or_default(), &vars)
            .map_err(|e| status_error("BAD_REQUEST", format!("render title failed: {e}")))?;
        let content = render_template(&tpl.content_template.unwrap_or_default(), &vars)
            .map_err(|e| status_error("BAD_REQUEST", format!("render content failed: {e}")))?;

        Ok(RenderNotificationTemplateResponse { title, content })
    }
}

/// 渲染 `{{var}}` 占位模板：单趟扫描，占位符名去变量集查值。
/// 空变量集仍检查"是否引用了占位符"（建/改模板时的语法试渲染即此路径）。
pub fn render_template(template: &str, vars: &BTreeMap<String, String>) -> Result<String, String> {
    if template.is_empty() {
        return Ok(String::new());
    }
    if vars.is_empty() {
        if let Some(start) = template.find("{{") {
            let rest = &template[start..];
            if let Some(end_rel) = rest.find("}}") {
                let name = rest[2..end_rel].trim();
                if !name.is_empty() {
                    return Err(format!("placeholder {{{{{name}}}}} has no value"));
                }
            }
        }
        return Ok(template.to_string());
    }

    let mut out = String::with_capacity(template.len() + 64);
    let mut rest = template;
    loop {
        let Some(start) = rest.find("{{") else {
            out.push_str(rest);
            break;
        };
        let after = &rest[start..];
        let Some(end_rel) = after.find("}}") else {
            // 未闭合的 {{ 按原文保留
            out.push_str(rest);
            break;
        };
        let name = after[2..end_rel].trim();
        out.push_str(&rest[..start]);
        if name.is_empty() {
            // {{ }} 空占位按原文保留
            out.push_str(&after[..end_rel + 2]);
        } else {
            let value = vars
                .get(name)
                .ok_or_else(|| format!("placeholder {{{{{name}}}}} has no value"))?;
            out.push_str(value);
        }
        rest = &after[end_rel + 2..];
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn render_replaces_known_placeholders() {
        let v = vars(&[("user", "杨"), ("code", "4821")]);
        assert_eq!(
            render_template("你好 {{user}}，验证码 {{code}}", &v).unwrap(),
            "你好 杨，验证码 4821"
        );
        // 同一占位符多次出现
        assert_eq!(render_template("{{user}}/{{user}}", &v).unwrap(), "杨/杨");
    }

    #[test]
    fn render_unknown_placeholder_errors_with_name() {
        let v = vars(&[("a", "1")]);
        let err = render_template("x {{b}} y", &v).unwrap_err();
        assert!(
            err.contains("{{b}}"),
            "error must name the placeholder: {err}"
        );
        // 空变量集同样报错（语法试渲染路径）
        assert!(render_template("x {{b}} y", &vars(&[])).is_err());
    }

    #[test]
    fn render_keeps_unclosed_and_empty_placeholders() {
        let v = vars(&[("a", "1")]);
        // 未闭合 {{ 按原文保留（模板正文含 JSON 片段是合法需求；正文里
        // 找不到成对 }} 时整段原样输出）
        assert_eq!(
            render_template("json {\"k\": \"{{a\"", &v).unwrap(),
            "json {\"k\": \"{{a\""
        );
        assert_eq!(render_template("x {{a y", &v).unwrap(), "x {{a y");
        // {{ }} 空占位按原文保留
        assert_eq!(render_template("{{ }} rest", &v).unwrap(), "{{ }} rest");
        // 空模板 → 空串
        assert_eq!(render_template("", &v).unwrap(), "");
    }
}
