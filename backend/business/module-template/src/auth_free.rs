/// The module's auth-free whitelist: (service_fq, method) pairs the
/// generator mounts on the PUBLIC router (no auth gate). The template
/// ships fully gated — every route answers 401 without a verified
/// token. To open an endpoint (a login-style route), add its
/// `(service_fq, method)` here AND annotate the rpc with the gnostic
/// `security: {}` doc marker (the admin contract's i_authentication is
/// the reference shape).
pub const AUTH_FREE: &[(&str, &str)] = &[];
