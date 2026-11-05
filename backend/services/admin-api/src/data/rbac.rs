//! RBAC entities: roles, role bindings, permissions, groups, menus, apis,
//! role metadata.

pub mod sys_roles {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_roles")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub name: String,
        pub code: String,
        /// 父角色（权限继承：子角色权限集必须 ⊆ 父角色已绑定权限集）；
        /// NULL = 根角色。
        pub parent_id: Option<u32>,
        #[sea_orm(default_value = false)]
        pub is_protected: Option<bool>,
        /// enum(SYSTEM,TEMPLATE,TENANT) default TENANT.
        #[sea_orm(column_name = "type")]
        pub type_column: Option<String>,
        /// enum(ALL,SELF,UNIT_ONLY,UNIT_AND_CHILD,SELECTED_UNITS) default ALL.
        pub data_scope: Option<String>,
        pub remark: Option<String>,
        pub description: Option<String>,
        /// enum(OFF,ON) default ON.
        pub status: Option<String>,
        #[sea_orm(default_value = 0)]
        pub sort_order: Option<u32>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub deleted_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_role_permissions {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_role_permissions")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub role_id: Option<u32>,
        pub permission_id: Option<u32>,
        /// enum(ALLOW,DENY) default ALLOW.
        pub effect: Option<String>,
        #[sea_orm(default_value = 0)]
        pub priority: Option<i32>,
        /// enum(OFF,ON) default ON.
        pub status: Option<String>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub deleted_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_role_metadata {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_role_metadata")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub role_id: Option<u32>,
        #[sea_orm(default_value = false)]
        pub is_template: Option<bool>,
        pub template_for: Option<String>,
        #[sea_orm(default_value = 1)]
        pub template_version: Option<i32>,
        pub last_synced_version: Option<i32>,
        pub last_synced_at: Option<chrono::NaiveDateTime>,
        /// enum(AUTO,MANUAL,BLOCKED) default AUTO.
        pub sync_policy: Option<String>,
        /// enum(PLATFORM,TENANT) default TENANT.
        pub scope: Option<String>,
        /// jsonb RoleOverride.
        pub custom_overrides: Option<Json>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub deleted_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_permissions {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_permissions")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub name: String,
        pub code: String,
        pub group_id: Option<u32>,
        pub description: Option<String>,
        /// enum(OFF,ON) default ON.
        pub status: Option<String>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub deleted_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_permission_groups {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_permission_groups")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub parent_id: Option<u32>,
        pub name: String,
        pub module: Option<String>,
        pub path: Option<String>,
        pub description: Option<String>,
        /// enum(OFF,ON) default ON.
        pub status: Option<String>,
        #[sea_orm(default_value = 0)]
        pub sort_order: Option<u32>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub deleted_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_permission_apis {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_permission_apis")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub permission_id: Option<u32>,
        pub api_id: Option<u32>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub deleted_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_permission_menus {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_permission_menus")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub permission_id: Option<u32>,
        pub menu_id: Option<u32>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub deleted_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_menus {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_menus")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub parent_id: Option<u32>,
        /// enum(CATALOG,MENU,BUTTON,EMBEDDED,LINK) default MENU.
        #[sea_orm(column_name = "type")]
        pub type_column: Option<String>,
        #[sea_orm(default_value = "")]
        pub path: Option<String>,
        pub redirect: Option<String>,
        pub alias: Option<String>,
        pub name: String,
        #[sea_orm(default_value = "")]
        pub component: Option<String>,
        /// jsonb MenuMeta.
        pub meta: Option<Json>,
        /// enum(DASHBOARD,OPM,SYSTEM,DICT,TENANT,PERMISSION,LOG,INTERNAL_MESSAGE,FILE,TASK).
        pub module: Option<String>,
        pub remark: Option<String>,
        /// enum(OFF,ON) default ON.
        pub status: Option<String>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub deleted_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_apis {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_apis")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub description: Option<String>,
        /// enum(DASHBOARD,OPM,SYSTEM,DICT,TENANT,PERMISSION,LOG,INTERNAL_MESSAGE,FILE,TASK).
        pub module: Option<String>,
        pub module_description: Option<String>,
        pub business_module: Option<String>,
        pub operation: Option<String>,
        pub path: Option<String>,
        pub method: Option<String>,
        /// enum(ADMIN,APP) default ADMIN.
        pub scope: Option<String>,
        /// enum(OFF,ON) default ON.
        pub status: Option<String>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub deleted_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_role_org_units {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_role_org_units")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub role_id: Option<u32>,
        pub org_unit_id: Option<u32>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub deleted_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
pub mod sys_role_field_permissions {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sys_role_field_permissions")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: u32,
        pub tenant_id: Option<u32>,
        pub role_id: Option<u32>,
        pub resource: Option<String>,
        pub field_name: Option<String>,
        pub created_by: Option<u32>,
        pub updated_by: Option<u32>,
        pub deleted_by: Option<u32>,
        pub created_at: Option<chrono::NaiveDateTime>,
        pub updated_at: Option<chrono::NaiveDateTime>,
        pub deleted_at: Option<chrono::NaiveDateTime>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
