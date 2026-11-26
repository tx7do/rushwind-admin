//! UserRepo — tenant-scoped
//! user queries, the credential verify with the dummy-hash timing
//! equalizer, identifier resolution, exists checks, and the credential
//! mutations (create/update password, history append).

use sea_orm::sea_query::Condition;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};

use crate::admin::data::Viewer;
use crate::admin::data::{
    sys_user_credentials as credentials, sys_user_org_units, sys_user_positions, sys_user_roles,
    sys_users as users,
};
use crate::admin::paging::PagingInput as _;
use crate::admin::state::{db_err, internal_error, status_error, StatusError};

pub struct UserRepo<'a> {
    pub db: &'a DatabaseConnection,
    pub viewer: Viewer,
}

pub struct NewUser<'a> {
    pub username: &'a str,
    pub nickname: Option<&'a str>,
    pub realname: Option<&'a str>,
    pub email: Option<&'a str>,
    pub mobile: Option<&'a str>,
    pub gender: Option<&'a str>,
    pub remark: Option<&'a str>,
    pub status: &'a str,
}

/// 分页 query 里的关系键过滤（org_unit_id(s)/position_id(s)/role_id(s)）。
/// 这些键不是 sys_users 的列——直接落通用过滤管线会拼出不存在列的 SQL；
/// 摘出后按各关系面解析用户 id 集，跨面求交集再钉成 id IN 谓词。
#[derive(Default)]
pub(crate) struct RelationFilter {
    org_unit_ids: Vec<i64>,
    position_ids: Vec<i64>,
    role_ids: Vec<i64>,
}

impl RelationFilter {
    fn is_empty(&self) -> bool {
        self.org_unit_ids.is_empty() && self.position_ids.is_empty() && self.role_ids.is_empty()
    }
}

/// 从 query JSON 摘除关系键并收集其值（键名 snake 归一，camel 拼写同收；
/// 值按查询值语法切分后取整数段，杂段忽略）。余键原样写回 query。
pub(crate) fn extract_relation_filter(query: &mut Option<String>) -> RelationFilter {
    let mut rel = RelationFilter::default();
    let Some(q) = query.as_deref() else {
        return rel;
    };
    let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(q) else {
        return rel;
    };
    let mut kept = serde_json::Map::new();
    for (key, value) in map {
        let field = rust_utils::stringcase::to_snake_case(&key);
        let ints_of = |sink: &mut Vec<i64>| {
            let items: Vec<String> = match &value {
                serde_json::Value::String(s) => rust_utils::query_parser::split_query_values(s)
                    .into_iter()
                    .map(String::from)
                    .collect(),
                serde_json::Value::Array(items) => items
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(String::from)
                            .unwrap_or_else(|| v.to_string())
                    })
                    .collect(),
                serde_json::Value::Number(n) => vec![n.to_string()],
                _ => Vec::new(),
            };
            for item in items {
                if let Ok(n) = item.trim().parse::<i64>() {
                    sink.push(n);
                }
            }
        };
        match field.as_str() {
            "org_unit_id" | "org_unit_ids" => ints_of(&mut rel.org_unit_ids),
            "position_id" | "position_ids" => ints_of(&mut rel.position_ids),
            "role_id" | "role_ids" => ints_of(&mut rel.role_ids),
            _ => {
                kept.insert(key, value);
            }
        }
    }
    *query = if kept.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&kept).unwrap_or_default())
    };
    rel
}

impl<'a> UserRepo<'a> {
    pub fn new(db: &'a DatabaseConnection, viewer: Viewer) -> Self {
        Self { db, viewer }
    }

    fn tenant_condition(&self) -> Condition {
        match self.viewer.tenant_scope() {
            Some(tid) => Condition::all().add(users::Column::TenantId.eq(tid)),
            None => Condition::all(),
        }
    }

    /// Paged listing within one explicit tenant, newest first:
    /// returns (rows, total). 关系键过滤在进通用管线前摘除解析。
    pub async fn paged_list(
        &self,
        tenant_id: i64,
        req: &crate::proto::pagination::PagingRequest,
    ) -> Result<(Vec<users::Model>, u64), StatusError> {
        let mut params = req.paging_params();
        let rel = extract_relation_filter(&mut params.query);
        let mut base = users::Entity::find()
            .filter(users::Column::TenantId.eq(tenant_id))
            .order_by_desc(users::Column::CreatedAt);
        if !rel.is_empty() {
            let merged = self.resolve_relation_user_ids(&rel).await?;
            if merged.is_empty() {
                return Ok((Vec::new(), 0));
            }
            base = base.filter(users::Column::Id.is_in(merged));
        }
        crate::admin::paging::fetch_paged(self.db, base, &params).await
    }

    /// 关系面 → 用户 id 集：面内并集（命中任一 id 即中），面间交集。
    /// 有关系过滤但交集为空时上层直接回空页。
    pub(crate) async fn resolve_relation_user_ids(
        &self,
        rel: &RelationFilter,
    ) -> Result<Vec<i64>, StatusError> {
        let mut lists: Vec<Vec<i64>> = Vec::new();
        if !rel.org_unit_ids.is_empty() {
            lists.push(
                sys_user_org_units::Entity::find()
                    .filter(sys_user_org_units::Column::OrgUnitId.is_in(&rel.org_unit_ids))
                    .all(self.db)
                    .await
                    .map_err(db_err)?
                    .into_iter()
                    .filter_map(|r| r.user_id)
                    .collect(),
            );
        }
        if !rel.position_ids.is_empty() {
            lists.push(
                sys_user_positions::Entity::find()
                    .filter(sys_user_positions::Column::PositionId.is_in(&rel.position_ids))
                    .all(self.db)
                    .await
                    .map_err(db_err)?
                    .into_iter()
                    .filter_map(|r| r.user_id)
                    .collect(),
            );
        }
        if !rel.role_ids.is_empty() {
            lists.push(
                sys_user_roles::Entity::find()
                    .filter(sys_user_roles::Column::RoleId.is_in(&rel.role_ids))
                    .all(self.db)
                    .await
                    .map_err(db_err)?
                    .into_iter()
                    .filter_map(|r| r.user_id)
                    .collect(),
            );
        }
        let Some(_) = lists.first() else {
            return Ok(Vec::new());
        };
        Ok(lists
            .into_iter()
            .reduce(|acc, ids| {
                let keep: std::collections::HashSet<i64> = ids.into_iter().collect();
                acc.into_iter().filter(|id| keep.contains(id)).collect()
            })
            .unwrap_or_default())
    }

    pub async fn count(&self) -> u64 {
        users::Entity::find()
            .filter(self.tenant_condition())
            .count(self.db)
            .await
            .unwrap_or(0)
    }

    pub async fn get_by_id(&self, id: i64) -> Result<users::Model, StatusError> {
        let mut query = users::Entity::find_by_id(id);
        if let Some(tid) = self.viewer.tenant_scope() {
            query = users::Entity::find_by_id(id).filter(users::Column::TenantId.eq(tid));
        }
        query
            .one(self.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| status_error("USER_NOT_FOUND", "user not found"))
    }

    pub async fn get_by_username(
        &self,
        username: &str,
    ) -> Result<Option<users::Model>, StatusError> {
        users::Entity::find()
            .filter(
                self.tenant_condition()
                    .add(users::Column::Username.eq(username)),
            )
            .one(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn get_by_email(&self, email: &str) -> Result<Option<users::Model>, StatusError> {
        users::Entity::find()
            .filter(self.tenant_condition().add(users::Column::Email.eq(email)))
            .one(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn get_by_mobile(&self, mobile: &str) -> Result<Vec<users::Model>, StatusError> {
        users::Entity::find()
            .filter(
                self.tenant_condition()
                    .add(users::Column::Mobile.eq(mobile)),
            )
            .all(self.db)
            .await
            .map_err(db_err)
    }

    /// FindUsernameByIdentifier (module:1168-1223): `@` → email,
    /// all-digits → mobile (ambiguous → 500), miss → input unchanged.
    pub async fn find_username_by_identifier(
        &self,
        tenant_id: i64,
        input: &str,
    ) -> Result<String, StatusError> {
        if input.is_empty() {
            return Ok(input.to_string());
        }
        let scoped = |col: users::Column| {
            Condition::all()
                .add(users::Column::TenantId.eq(tenant_id))
                .add(col.eq(input))
        };
        if input.contains('@') {
            let row = users::Entity::find()
                .filter(scoped(users::Column::Email))
                .one(self.db)
                .await
                .map_err(db_err)?;
            return Ok(row.map(|u| u.username).unwrap_or_else(|| input.to_string()));
        }
        if input.bytes().all(|b| b.is_ascii_digit()) {
            let rows = users::Entity::find()
                .filter(scoped(users::Column::Mobile))
                .all(self.db)
                .await
                .map_err(db_err)?;
            if rows.len() > 1 {
                return Err(internal_error("ambiguous account identifier"));
            }
            return Ok(rows
                .first()
                .map(|u| u.username.clone())
                .unwrap_or_else(|| input.to_string()));
        }
        Ok(input.to_string())
    }

    /// Force-tenant-scoped list (tenants never see across).
    pub async fn list_tenant(&self, tenant_id: i64) -> Result<Vec<users::Model>, StatusError> {
        users::Entity::find()
            .filter(users::Column::TenantId.eq(tenant_id))
            .order_by_desc(users::Column::CreatedAt)
            .all(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn create(
        &self,
        req: NewUser<'a>,
        operator_id: i64,
    ) -> Result<users::Model, StatusError> {
        let tenant_id = self.viewer.stamp_tenant(None);
        users::ActiveModel {
            tenant_id: Set(tenant_id),
            username: Set(req.username.to_string()),
            nickname: Set(req.nickname.map(String::from)),
            realname: Set(req.realname.map(String::from)),
            email: Set(req.email.map(String::from)),
            mobile: Set(req.mobile.map(String::from)),
            gender: Set(req.gender.map(String::from)),
            remark: Set(req.remark.map(String::from)),
            status: Set(Some(req.status.to_string())),
            created_by: Set(Some(operator_id)),
            created_at: Set(Some(crate::admin::data::now())),
            updated_at: Set(Some(crate::admin::data::now())),
            ..Default::default()
        }
        .insert(self.db)
        .await
        .map_err(db_err)
    }

    /// recordUserLastLogin — non-blocking best-effort update.
    pub async fn record_last_login(&self, user_id: i64, ip: &str) {
        if let Ok(Some(row)) = users::Entity::find_by_id(user_id).one(self.db).await {
            let mut a: users::ActiveModel = row.into();
            a.last_login_at = Set(Some(crate::admin::data::now()));
            a.last_login_ip = Set(Some(ip.to_string()));
            let _ = a.update(self.db).await;
        }
    }

    // ----- credentials  -----

    pub async fn find_credential(
        &self,
        tenant_id: i64,
        identifier: &str,
    ) -> Result<Option<credentials::Model>, StatusError> {
        credentials::Entity::find()
            .filter(
                Condition::all()
                    .add(credentials::Column::TenantId.eq(tenant_id))
                    .add(credentials::Column::IdentityType.eq("USERNAME"))
                    .add(credentials::Column::Identifier.eq(identifier)),
            )
            .one(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn find_credential_by_email(
        &self,
        identifier: &str,
    ) -> Result<Option<credentials::Model>, StatusError> {
        credentials::Entity::find()
            .filter(
                Condition::all()
                    .add(credentials::Column::IdentityType.eq("EMAIL"))
                    .add(credentials::Column::Identifier.eq(identifier)),
            )
            .one(self.db)
            .await
            .map_err(db_err)
    }

    pub async fn upsert_credential(
        &self,
        tenant_id: i64,
        user_id: i64,
        identifier: &str,
        bcrypt_hash: &str,
        operator_id: i64,
    ) -> Result<(), StatusError> {
        let existing = self.find_credential(tenant_id, identifier).await?;
        match existing {
            Some(row) => {
                let mut a: credentials::ActiveModel = row.into();
                a.credential = Set(bcrypt_hash.to_string());
                a.updated_at = Set(Some(crate::admin::data::now()));
                a.updated_by = Set(Some(operator_id));
                a.update(self.db).await.map_err(db_err)?;
            }
            None => {
                credentials::ActiveModel {
                    tenant_id: Set(Some(tenant_id)),
                    user_id: Set(Some(user_id)),
                    identity_type: Set(Some("USERNAME".into())),
                    identifier: Set(identifier.to_string()),
                    credential_type: Set(Some("PASSWORD_HASH".into())),
                    credential: Set(bcrypt_hash.to_string()),
                    is_primary: Set(Some(true)),
                    status: Set(Some("ENABLED".into())),
                    created_by: Set(Some(operator_id)),
                    created_at: Set(Some(crate::admin::data::now())),
                    updated_at: Set(Some(crate::admin::data::now())),
                    ..Default::default()
                }
                .insert(self.db)
                .await
                .map_err(db_err)?;
            }
        }
        Ok(())
    }

    /// The password-history read/append over `extra_info.password_history`
    /// (module).
    pub fn read_history(cred: &credentials::Model) -> Vec<String> {
        cred.extra_info
            .as_ref()
            .and_then(|v| v.get("password_history"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    pub fn write_history(
        old_hash: &str,
        mut history: Vec<String>,
        keep: usize,
    ) -> serde_json::Value {
        history.push(old_hash.to_string());
        let cut = history.len().saturating_sub(keep);
        history.drain(..cut);
        serde_json::json!({ "password_history": history })
    }

    /// Role bindings of a user (sys_user_roles).
    pub async fn list_role_ids(&self, user_id: i64) -> Result<Vec<i64>, StatusError> {
        Ok(sys_user_roles::Entity::find()
            .filter(sys_user_roles::Column::UserId.eq(user_id))
            .all(self.db)
            .await
            .map_err(db_err)?
            .iter()
            .filter_map(|r| r.role_id)
            .collect())
    }

    /// Org-unit bindings of a user (sys_user_org_units).
    pub async fn list_org_unit_ids(&self, user_id: i64) -> Result<Vec<i64>, StatusError> {
        Ok(sys_user_org_units::Entity::find()
            .filter(sys_user_org_units::Column::UserId.eq(user_id))
            .all(self.db)
            .await
            .map_err(db_err)?
            .iter()
            .filter_map(|r| r.org_unit_id)
            .collect())
    }

    /// Position bindings of a user (sys_user_positions).
    pub async fn list_position_ids(&self, user_id: i64) -> Result<Vec<i64>, StatusError> {
        Ok(sys_user_positions::Entity::find()
            .filter(sys_user_positions::Column::UserId.eq(user_id))
            .all(self.db)
            .await
            .map_err(db_err)?
            .iter()
            .filter_map(|r| r.position_id)
            .collect())
    }

    pub async fn delete_with_relations(
        &self,
        tenant_id: i64,
        user_id: i64,
    ) -> Result<(), StatusError> {
        credentials::Entity::delete_many()
            .filter(
                Condition::all()
                    .add(credentials::Column::TenantId.eq(tenant_id))
                    .add(credentials::Column::UserId.eq(user_id)),
            )
            .exec(self.db)
            .await
            .map_err(db_err)?;
        sys_user_roles::Entity::delete_many()
            .filter(sys_user_roles::Column::UserId.eq(user_id))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        sys_user_org_units::Entity::delete_many()
            .filter(sys_user_org_units::Column::UserId.eq(user_id))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        sys_user_positions::Entity::delete_many()
            .filter(sys_user_positions::Column::UserId.eq(user_id))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        users::Entity::delete_by_id(user_id)
            .exec(self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 关系键摘除：snake/camel 拼写同收（键名经 to_snake_case 归一），
    /// 值按查询值语法切分取整数段（杂段忽略），余键原样保留在 query 里
    /// 交给通用过滤管线。
    #[test]
    fn relation_keys_extracted_and_stripped_from_query() {
        let mut query =
            Some(r#"{"orgUnitId":"7","role_ids":"3|5","nickname__contains":"al"}"#.to_string());
        let rel = extract_relation_filter(&mut query);
        assert_eq!(rel.org_unit_ids, vec![7]);
        assert_eq!(rel.role_ids, vec![3, 5]);
        assert!(rel.position_ids.is_empty());
        let kept = query.expect("remaining query string kept");
        assert!(
            kept.contains("nickname__contains"),
            "unrelated key must survive: {kept}"
        );
        assert!(!kept.contains("orgUnitId"), "relation key stripped: {kept}");
        assert!(!kept.contains("role_ids"), "relation key stripped: {kept}");
    }

    /// 无 query / 非对象形态（框架管线自己的解析路径处理）原样放行。
    #[test]
    fn absent_or_unshaped_query_yields_empty_filter() {
        let mut none = None;
        assert!(extract_relation_filter(&mut none).is_empty());
        assert!(none.is_none());

        let mut plain = Some(r#"{"nickname":"al"}"#.to_string());
        let rel = extract_relation_filter(&mut plain);
        assert!(rel.is_empty());
        assert_eq!(plain.as_deref(), Some(r#"{"nickname":"al"}"#));
    }
}
