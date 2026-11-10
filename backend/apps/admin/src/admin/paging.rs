//! The deployment bridge to the framework paging pipeline
//! (`rushwind-storage-seaorm-support`): the generated `PagingRequest`
//! resolves into the framework's [`Params`], and the pipeline itself —
//! filter binding by column kind, the orderBy spellings, ordering,
//! slicing — lives there. The repository call sites keep passing the
//! request verbatim.

// The `PagingInput` impl for the generated PagingRequest lives in the
// proto crate — the orphan rule pins the impl to the type's owning
// crate — so the repositories' `fetch_paged(db, base, req)` call sites
// compile unchanged.

pub use rushwind_storage_seaorm_support::paging::{column_kind, fetch_paged, Kind};

/// The frozen kind oracle: the hand-written name lists the runtime
/// table replaced. Each listed column's runtime classification must
/// keep matching its frozen entry — a schema migration flipping a
/// column's type flips this test. Unlisted names bind as text and
/// stay unasserted.
#[cfg(test)]
mod kind_oracle {
    use rushwind_storage_seaorm_support::paging::{column_kind, Kind};

    const NUMERIC: &[&str] = &[
        "id",
        "tenant_id",
        "user_id",
        "role_id",
        "permission_id",
        "menu_id",
        "api_id",
        "group_id",
        "parent_id",
        "org_unit_id",
        "position_id",
        "message_id",
        "recipient_user_id",
        "sender_id",
        "category_id",
        "type_id",
        "entry_id",
        "plan_id",
        "leader_id",
        "contact_user_id",
        "created_by",
        "updated_by",
        "deleted_by",
        "assigned_by",
        "admin_user_id",
        "script_id",
        "operator_id",
        "membership_id",
        "policy_id",
        "reports_to_position_id",
        "legal_entity_org_id",
        "headcount",
        "level",
        "priority",
        "template_version",
        "last_synced_version",
        "numeric_value",
        "quota_value",
        "size",
        "smtp_port",
        "latency_ms",
        "status_code",
        "risk_score",
        "affected_rows",
        "duration_ms",
        "sort_order",
    ];
    const BOOL: &[&str] = &[
        "is_enabled",
        "is_primary",
        "is_protected",
        "is_default",
        "is_built_in",
        "is_template",
        "enable",
        "success",
        "critical",
        "is_legal_entity",
        "is_key_position",
        "data_masked",
    ];

    fn frozen(name: &str) -> Option<Kind> {
        if NUMERIC.contains(&name) {
            Some(Kind::Number)
        } else if BOOL.contains(&name) {
            Some(Kind::Bool)
        } else {
            None
        }
    }

    /// The runtime classification mirrors the frozen oracle for every
    /// oracle-listed column of every entity in the data layer.
    #[test]
    fn runtime_kinds_match_the_frozen_oracle() {
        macro_rules! check {
            ($entity:ty) => {
                for col in <<$entity as sea_orm::EntityTrait>::Column as sea_orm::Iterable>::iter()
                {
                    let name = sea_orm::sea_query::Iden::to_string(&col);
                    if let Some(expected) = frozen(&name) {
                        let got = column_kind::<$entity>(&name);
                        assert_eq!(
                            std::mem::discriminant(&got),
                            std::mem::discriminant(&expected),
                            "kind drift on {name}"
                        );
                    }
                }
            };
        }
        check!(crate::admin::data::files::Entity);
        check!(crate::admin::data::internal_message_categories::Entity);
        check!(crate::admin::data::internal_message_recipients::Entity);
        check!(crate::admin::data::internal_messages::Entity);
        check!(crate::admin::data::sys_access_keys::Entity);
        check!(crate::admin::data::sys_api_audit_logs::Entity);
        check!(crate::admin::data::sys_apis::Entity);
        check!(crate::admin::data::sys_configs::Entity);
        check!(crate::admin::data::sys_data_access_audit_logs::Entity);
        check!(crate::admin::data::sys_dict_entries::Entity);
        check!(crate::admin::data::sys_dict_entry_i18n::Entity);
        check!(crate::admin::data::sys_dict_types::Entity);
        check!(crate::admin::data::sys_languages::Entity);
        check!(crate::admin::data::sys_login_audit_logs::Entity);
        check!(crate::admin::data::sys_login_policies::Entity);
        check!(crate::admin::data::sys_menus::Entity);
        check!(crate::admin::data::sys_notification_channels::Entity);
        check!(crate::admin::data::sys_operation_audit_logs::Entity);
        check!(crate::admin::data::sys_org_units::Entity);
        check!(crate::admin::data::sys_permission_apis::Entity);
        check!(crate::admin::data::sys_permission_audit_logs::Entity);
        check!(crate::admin::data::sys_permission_groups::Entity);
        check!(crate::admin::data::sys_permission_menus::Entity);
        check!(crate::admin::data::sys_permissions::Entity);
        check!(crate::admin::data::sys_plan_modules::Entity);
        check!(crate::admin::data::sys_plan_quotas::Entity);
        check!(crate::admin::data::sys_plans::Entity);
        check!(crate::admin::data::sys_policy_evaluation_logs::Entity);
        check!(crate::admin::data::sys_positions::Entity);
        check!(crate::admin::data::sys_role_field_permissions::Entity);
        check!(crate::admin::data::sys_role_metadata::Entity);
        check!(crate::admin::data::sys_role_org_units::Entity);
        check!(crate::admin::data::sys_role_permissions::Entity);
        check!(crate::admin::data::sys_roles::Entity);
        check!(crate::admin::data::sys_script_logs::Entity);
        check!(crate::admin::data::sys_scripts::Entity);
        check!(crate::admin::data::sys_tasks::Entity);
        check!(crate::admin::data::sys_tenants::Entity);
        check!(crate::admin::data::sys_user_credentials::Entity);
        check!(crate::admin::data::sys_user_mfa_factors::Entity);
        check!(crate::admin::data::sys_user_roles::Entity);
        check!(crate::admin::data::sys_users::Entity);
    }

    /// The schema-typed binding stays schema-typed: sys_plans carries
    /// `version` as a text label — the one column the retired
    /// name-keyed table misbound — and it must keep binding as text.
    #[test]
    fn plan_version_column_binds_as_text() {
        assert_eq!(
            std::mem::discriminant(&column_kind::<crate::admin::data::sys_plans::Entity>(
                "version"
            )),
            std::mem::discriminant(&Kind::Text)
        );
    }
}
