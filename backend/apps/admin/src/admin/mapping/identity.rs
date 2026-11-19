//! The identity-domain wire enums: users, roles, positions, tenants,
//! plans and org units. `role_type_of` rides the `permission` proto
//! package — the wire Role lives there.

enum_map!(user_gender_of => "identity.service.v1.User.Gender");

enum_map!(user_status_of => "identity.service.v1.User.Status");

enum_map!(role_scope_of + role_scope_str => "identity.service.v1.DataScope");

enum_map!(role_type_of + role_type_str => "permission.service.v1.Role.Type");

enum_map!(position_type_of + position_type_str => "identity.service.v1.Position.Type");

enum_map!(tenant_status_of + tenant_status_str => "identity.service.v1.Tenant.Status");

enum_map!(tenant_audit_status_of + tenant_audit_status_str => "identity.service.v1.Tenant.AuditStatus");

enum_map!(tenant_type_of + tenant_type_str => "identity.service.v1.Tenant.Type");

enum_map!(plan_version_of + plan_version_str => "identity.service.v1.Plan.Version");

enum_map!(plan_expiry_policy_of + plan_expiry_policy_str => "identity.service.v1.Plan.ExpiryPolicy");

enum_map!(plan_quota_type_of + plan_quota_type_str => "identity.service.v1.PlanQuota.QuotaType");

enum_map!(org_unit_type_of + org_unit_type_str => "identity.service.v1.OrgUnit.Type");
