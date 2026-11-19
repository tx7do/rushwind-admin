//! The authentication-domain wire enums: the login-policy gate types
//! and their verification methods.

enum_map!(login_policy_type_of + login_policy_type_str => "authentication.service.v1.LoginPolicy.Type");

enum_map!(login_policy_method_of + login_policy_method_str => "authentication.service.v1.LoginPolicy.Method");
