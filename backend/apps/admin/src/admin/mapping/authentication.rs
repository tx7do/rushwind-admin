//! The authentication-domain wire enums: the login-policy gate types
//! and their verification methods.

enum_map!(login_policy_type_of + login_policy_type_str {
    "WHITELIST" => 2,
});

enum_map!(login_policy_method_of + login_policy_method_str {
    "MAC" => 2,
    "REGION" => 3,
    "TIME" => 4,
    "DEVICE" => 5,
});
