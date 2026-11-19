//! The audit-domain wire enums: login audits, operation audits,
//! permission grants and data-access records.

enum_map!(login_audit_action_of => "audit.service.v1.LoginAuditLog.ActionType");

enum_map!(login_audit_status_of => "audit.service.v1.LoginAuditLog.Status");

enum_map!(login_audit_method_of => "audit.service.v1.LoginAuditLog.LoginMethod");

enum_map!(login_audit_risk_level_of => "audit.service.v1.LoginAuditLog.RiskLevel");

enum_map!(operation_audit_action_of => "audit.service.v1.OperationAuditLog.ActionType");

enum_map!(permission_audit_action_of => "audit.service.v1.PermissionAuditLog.ActionType");

enum_map!(operation_audit_sensitive_level_of => "audit.service.v1.SensitiveLevel");

enum_map!(data_access_type_of => "audit.service.v1.DataAccessAuditLog.AccessType");

enum_map!(data_access_sensitive_level_of => "audit.service.v1.SensitiveLevel");
