//! UserRepo — tenant-scoped
//! user queries, the credential verify with the dummy-hash timing
//! equalizer, identifier resolution, exists checks, and the credential
//! mutations (create/update password, history append).

use sea_orm::sea_query::Condition;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};

use crate::data::Viewer;
use crate::data::{sys_user_credentials as credentials, sys_user_roles, sys_users as users};
use crate::state::{db_err, internal_error, status_error, StatusError};

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
    /// returns (rows, total).
    pub async fn paged_list(
        &self,
        tenant_id: i64,
        req: &proto::proto::pagination::PagingRequest,
    ) -> Result<(Vec<users::Model>, u64), StatusError> {
        crate::paging::fetch_paged(
            self.db,
            users::Entity::find()
                .filter(users::Column::TenantId.eq(tenant_id))
                .order_by_desc(users::Column::CreatedAt),
            req,
        )
        .await
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
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
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
            a.last_login_at = Set(Some(crate::data::now()));
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
                a.updated_at = Set(Some(crate::data::now()));
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
                    created_at: Set(Some(crate::data::now())),
                    updated_at: Set(Some(crate::data::now())),
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
    pub async fn list_role_ids(&self, user_id: i64) -> Result<Vec<u64>, StatusError> {
        Ok(sys_user_roles::Entity::find()
            .filter(sys_user_roles::Column::UserId.eq(user_id))
            .all(self.db)
            .await
            .map_err(db_err)?
            .iter()
            .filter_map(|r| r.role_id.map(|v| v as u64))
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
        users::Entity::delete_by_id(user_id)
            .exec(self.db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
