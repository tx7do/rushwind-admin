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

use crate::admin::state::{
    db_err, not_found, operator_of, require_data, status_error, AppState, StatusError,
};
use crate::proto::notification::service::v1::{
    CreateNotificationTemplateRequest, DeleteNotificationTemplateRequest,
    GetNotificationTemplateRequest, ListNotificationTemplateResponse, NotificationTemplate,
    RenderNotificationTemplateRequest, RenderNotificationTemplateResponse,
    UpdateNotificationTemplateRequest,
};
use crate::proto::pagination::PagingRequest;
use pbjson_types::Empty;

fn template_proto(
    r: crate::admin::data::sys_notification_templates::Model,
) -> NotificationTemplate {
    NotificationTemplate {
        id: Some(r.id as u64),
        name: r.name,
        code: r.code,
        title_template: r.title_template,
        content_template: r.content_template,
        is_enabled: r.is_enabled,
        remark: r.remark,
        created_at: r.created_at.and_then(crate::admin::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::admin::state::naive_to_ts),
    }
}

/// 字段写入统一过 update_mask 白名单门（字段级更新语义，无掩码即存量
/// if-let-Some 全量提交语义不变）。全部可空列走 opt 形态：掩码选中 +
/// 载荷 None = 显式清列；无掩码 None 不动列。
fn apply_fields(
    a: &mut crate::data::sys_notification_templates::ActiveModel,
    data: &NotificationTemplate,
    mask: &crate::fieldmask::FieldMask,
) {
    crate::set_masked!(opt mask, a, name, data.name, |v: &String| Some(v.clone()));
    crate::set_masked!(opt mask, a, code, data.code, |v: &String| Some(v.clone()));
    crate::set_masked!(opt mask, a, title_template, data.title_template, |v: &String| Some(
        v.clone()
    ));
    crate::set_masked!(
        opt mask,
        a,
        content_template,
        data.content_template,
        |v: &String| Some(v.clone())
    );
    crate::set_masked!(opt mask, a, is_enabled, data.is_enabled, |v: &bool| Some(
        *v
    ));
    crate::set_masked!(opt mask, a, remark, data.remark, |v: &String| Some(
        v.clone()
    ));
}

pub struct NotificationTemplateService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::services::NotificationTemplateServiceHandlers
    for NotificationTemplateService
{
    /// 模板列表（分页，contains 搜索）。
    async fn list_notification_template(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListNotificationTemplateResponse, StatusError> {
        let repo = crate::admin::data::repos::NotificationTemplateRepo::new(&self.state.db);
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
        let repo = crate::admin::data::repos::NotificationTemplateRepo::new(&self.state.db);
        let id = match &req.query_by {
            Some(crate::proto::notification::service::v1::get_notification_template_request::QueryBy::Id(id)) => *id,
            Some(crate::proto::notification::service::v1::get_notification_template_request::QueryBy::Code(code)) => {
                let row = repo
                    .get_by_code(code)
                    .await?
                    .ok_or_else(|| not_found("notification template"))?;
                return Ok(template_proto(row));
            }
            None => 0,
        };
        if id != 0 {
            return Ok(template_proto(repo.get_by_id(id as i64).await?));
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
        let repo = crate::admin::data::repos::NotificationTemplateRepo::new(&self.state.db);
        if let Some(code) = data.code.as_deref().filter(|c| !c.is_empty()) {
            if repo.get_by_code(code).await?.is_some() {
                return Err(status_error("CONFLICT", "template code already exists"));
            }
        }
        let mut a = crate::admin::data::sys_notification_templates::ActiveModel {
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        };
        apply_fields(&mut a, &data, &crate::fieldmask::FieldMask::unmasked());
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

    /// 更新模板（请求体 {data:{...}, updateMask?}）：字段写入统一过
    /// update_mask 白名单门——有掩码只有选中字段允许动（Some 写值、None
    /// 显式清列），无掩码即存量全量提交语义。code 唯一性冲突校验保留
    /// 原位（门只管"载荷字段→列"的写面）。
    async fn update_notification_template(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateNotificationTemplateRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        let repo = crate::admin::data::repos::NotificationTemplateRepo::new(&self.state.db);
        let row = repo.get_by_id(req.id as i64).await?;
        if let Some(code) = data.code.as_deref().filter(|c| !c.is_empty()) {
            if let Some(other) = repo.get_by_code(code).await? {
                if other.id != req.id as i64 {
                    return Err(status_error("CONFLICT", "template code already exists"));
                }
            }
        }
        let mut a: crate::data::sys_notification_templates::ActiveModel = row.into();
        let mask = crate::fieldmask::FieldMask::from(req.update_mask.as_ref());
        apply_fields(&mut a, &data, &mask);
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
        let repo = crate::admin::data::repos::NotificationTemplateRepo::new(&self.state.db);
        repo.get_by_id(req.id as i64).await?;
        repo.delete_by_id(req.id as i64).await?;
        Ok(Empty {})
    }

    /// 试渲染：变量集给了什么就渲染什么，模板引用了变量集之外的占位符
    /// → 400 带占位符名（管理页预览用，不产生投递）。
    async fn render_notification_template(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: RenderNotificationTemplateRequest,
    ) -> Result<RenderNotificationTemplateResponse, StatusError> {
        let repo = crate::admin::data::repos::NotificationTemplateRepo::new(&self.state.db);
        let tpl = repo.get_by_id(req.id as i64).await?;
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
    use crate::data::sys_notification_templates as templates;
    use crate::testsupport::{ctx, test_state};
    use crate::proto::gen::services::NotificationTemplateServiceHandlers as _;
    use sea_orm::{EntityTrait, Set};

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    async fn plant(db: &sea_orm::DatabaseConnection, name: &str, code: &str) -> templates::Model {
        templates::ActiveModel {
            name: Set(Some(name.into())),
            code: Set(Some(code.into())),
            title_template: Set(Some("[RushWind] {{title}}".into())),
            content_template: Set(Some("正文 {{code}}".into())),
            is_enabled: Set(Some(true)),
            remark: Set(Some("原始备注".into())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn row_by_id(db: &sea_orm::DatabaseConnection, id: i64) -> templates::Model {
        templates::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn update_req(
        id: u64,
        data: NotificationTemplate,
        paths: &[&str],
    ) -> UpdateNotificationTemplateRequest {
        UpdateNotificationTemplateRequest {
            id,
            data: Some(data),
            update_mask: Some(pbjson_types::FieldMask {
                paths: paths.iter().map(|s| s.to_string()).collect(),
            }),
        }
    }

    /// update_mask 白名单门（字段级更新语义）：掩码只选 name 时，载荷里
    /// 即便带了 remark/is_enabled 也不许动——部分提交不再误写未选中列。
    #[tokio::test]
    async fn mask_gates_unlisted_scalar_fields() {
        let state = test_state().await;
        let svc = NotificationTemplateService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, "旧模板", "tpl_old").await;

        svc.update_notification_template(
            ctx(0, 9),
            update_req(
                row.id as u64,
                NotificationTemplate {
                    name: Some("新模板".into()),
                    remark: Some("越权备注".into()),
                    is_enabled: Some(false),
                    ..Default::default()
                },
                &["name"],
            ),
        )
        .await
        .unwrap();

        let fresh = row_by_id(db, row.id).await;
        assert_eq!(fresh.name.as_deref(), Some("新模板"), "masked name lands");
        assert_eq!(
            fresh.remark.as_deref(),
            Some("原始备注"),
            "unmasked remark is not writable through the gate"
        );
        assert_eq!(
            fresh.is_enabled,
            Some(true),
            "unmasked is_enabled untouched"
        );
    }

    /// 掩码选中 + 载荷 None = 显式清列（字段级清值通道）；无掩码时
    /// None 仍是"不动"（全量提交语义不回退）。
    #[tokio::test]
    async fn masked_none_clears_nullable_column() {
        let state = test_state().await;
        let svc = NotificationTemplateService {
            state: state.clone(),
        };
        let db = &state.db;
        let row = plant(db, "模板", "tpl_demo").await;

        // 无掩码：None 不动列。
        svc.update_notification_template(
            ctx(0, 9),
            UpdateNotificationTemplateRequest {
                id: row.id as u64,
                data: Some(NotificationTemplate {
                    name: Some("改名".into()),
                    ..Default::default()
                }),
                update_mask: None,
            },
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert_eq!(
            fresh.remark.as_deref(),
            Some("原始备注"),
            "no mask keeps the whole-submit semantics"
        );

        // 掩码选中：None 清列。
        svc.update_notification_template(
            ctx(0, 9),
            update_req(row.id as u64, NotificationTemplate::default(), &["remark"]),
        )
        .await
        .unwrap();
        let fresh = row_by_id(db, row.id).await;
        assert!(
            fresh.remark.is_none(),
            "masked None explicitly clears the column"
        );
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
