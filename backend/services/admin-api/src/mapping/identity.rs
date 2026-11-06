//! The identity-domain wire enums: users, roles, positions, tenants,
//! plans and org units — every table whose wire enum lives in the
//! `identity` proto package.

enum_map!(user_gender_of {
    "MALE" => 1,
    "FEMALE" => 2,
});

enum_map!(user_status_of {
    "NORMAL" => 1,
    "PENDING" => 2,
    "LOCKED" => 3,
    "EXPIRED" => 4,
    "CLOSED" => 9,
});

enum_map!(role_scope_of {
    "SELF" => 1,
    "UNIT_ONLY" => 2,
    "UNIT_AND_CHILD" => 3,
    "SELECTED_UNITS" => 4,
});

enum_map!(role_type_of {
    "SYSTEM" => 1,
    "TEMPLATE" => 2,
});

enum_map!(position_type_of {
    "MANAGER" => 1,
    "LEAD" => 2,
    "INTERN" => 3,
    "CONTRACT" => 4,
    "OTHER" => 5,
});

enum_map!(tenant_status_of + tenant_status_str {
    "OFF" => 1,
    "EXPIRED" => 2,
    "FREEZE" => 3,
});

enum_map!(tenant_type_of + tenant_type_str {
    "TRIAL" => 0,
    "INTERNAL" => 2,
    "PARTNER" => 3,
    "CUSTOM" => 4,
});

enum_map!(plan_version_of + plan_version_str {
    "STANDARD" => 1,
    "ENTERPRISE" => 2,
});

enum_map!(plan_expiry_policy_of + plan_expiry_policy_str {
    "BLOCK_LOGIN" => 1,
    "FREEZE" => 2,
});

enum_map!(plan_quota_type_of + plan_quota_type_str {
    "STORAGE" => 1,
    "API_CALL" => 2,
});

enum_map!(org_unit_type_of + org_unit_type_str {
    "COMPANY" => 1,
    "DIVISION" => 2,
    "TEAM" => 3,
    "PROJECT" => 4,
    "COMMITTEE" => 5,
    "REGION" => 6,
    "OTHER" => 7,
});
