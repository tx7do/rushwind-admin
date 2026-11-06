//! The audit-domain wire enums: login audits, operation audits,
//! permission grants and data-access records.

enum_map!(login_audit_action_of {
    "LOGOUT" => 1,
    "SESSION_EXPIRED" => 2,
    "KICKED_OUT" => 3,
    "PASSWORD_RESET" => 4,
});

enum_map!(login_audit_status_of {
    "FAILED" => 1,
    "PARTIAL" => 2,
    "LOCKED" => 3,
});

enum_map!(login_audit_method_of {
    "SMS_CODE" => 1,
    "QR_CODE" => 2,
    "OIDC_SOCIAL" => 3,
    "BIOMETRIC" => 4,
    "FIDO2" => 5,
});

enum_map!(login_audit_risk_level_of {
    "MEDIUM" => 1,
    "HIGH" => 2,
});

enum_map!(operation_audit_action_of {
    "CREATE" => 0,
    "UPDATE" => 1,
    "DELETE" => 2,
    "READ" => 3,
    "ASSIGN" => 4,
    "UNASSIGN" => 5,
    "EXPORT" => 6,
    "IMPORT" => 7,
});

enum_map!(permission_audit_action_of {
    "GRANT" => 0,
    "REVOKE" => 1,
    "UPDATE" => 2,
    "RESET" => 3,
    "CREATE" => 4,
    "DELETE" => 5,
    "ASSIGN" => 6,
    "UNASSIGN" => 7,
    "BULK_GRANT" => 8,
    "BULK_REVOKE" => 9,
    "EXPIRE" => 10,
    "SUSPEND" => 11,
    "RESUME" => 12,
    "ROLLBACK" => 13,
    "OTHER" => 15,
});

enum_map!(operation_audit_sensitive_level_of {
    "PUBLIC" => 0,
    "INTERNAL" => 1,
    "CONFIDENTIAL" => 2,
});

enum_map!(data_access_type_of {
    "INSERT" => 1,
    "UPDATE" => 2,
    "DELETE" => 3,
    "VIEW" => 4,
    "BULK_READ" => 5,
    "EXPORT" => 6,
    "IMPORT" => 7,
    "DDL_CREATE" => 8,
    "DDL_ALTER" => 9,
    "DDL_DROP" => 10,
    "METADATA_READ" => 11,
    "SCAN" => 12,
    "ADMIN_OPERATION" => 13,
    "OTHER" => 14,
});

enum_map!(data_access_sensitive_level_of {
    "INTERNAL" => 1,
    "CONFIDENTIAL" => 2,
    "SECRET" => 3,
});
