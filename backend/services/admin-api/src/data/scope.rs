//! The viewer context — the standalone tenancy rule. Every repository receives a [`Viewer`] and
//! derives its query predicates from it; there is exactly one place that
//! decides tenancy filtering.
//!
//! * platform viewer (`tid == 0`): no tenant predicate (rows of all
//!   tenants visible);
//! * tenant viewer: every query/update/delete is constrained to
//!   `tenant_id = tid`, creates are force-stamped, cross-tenant writes
//!   are denied;
//! * system viewer (startup seeds, token-cleanup jobs): bypasses all
//!   tenant predicates.

use rushwind_http_binding::ctx::RequestContext;

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum ViewerKind {
    /// No credentials (public routes).
    Noop,
    /// A verified user context: platform when tenant 0.
    User,
    /// Bypasses tenancy (seeds, internal jobs).
    System,
}

/// The viewer context; the methods a service has not migrated to yet
/// stay as part of the data-layer API.
#[derive(Clone, Debug, Copy)]
pub struct Viewer {
    pub kind: ViewerKind,
    pub user_id: i64,
    pub tenant_id: i64,
}

impl Viewer {
    /// The noop viewer (public routes).
    pub fn noop() -> Self {
        Self {
            kind: ViewerKind::Noop,
            user_id: 0,
            tenant_id: 0,
        }
    }

    /// The system viewer.
    pub fn system() -> Self {
        Self {
            kind: ViewerKind::System,
            user_id: 0,
            tenant_id: 0,
        }
    }

    /// From the request's claim bag; `None` claims yield the noop viewer.
    pub fn from_ctx(ctx: &RequestContext) -> Self {
        match ctx
            .claims
            .as_ref()
            .and_then(crate::token::UserTokenPayload::from_claims)
        {
            Some(payload) => Self {
                kind: ViewerKind::User,
                user_id: payload.user_id,
                tenant_id: payload.tenant_id,
            },
            None => Self::noop(),
        }
    }

    /// Tenant enforcement — `Some(tenant)` when rows must be
    /// constrained, `None` for platform/system/noop wide reads.
    pub fn tenant_scope(&self) -> Option<i64> {
        match self.kind {
            ViewerKind::User if self.tenant_id > 0 => Some(self.tenant_id),
            _ => None,
        }
    }

    /// Create stamping: tenant viewers force their tenant onto new rows.
    pub fn stamp_tenant(&self, requested: Option<i64>) -> Option<i64> {
        match self.kind {
            ViewerKind::User if self.tenant_id > 0 => Some(self.tenant_id),
            _ => requested.or(Some(0)),
        }
    }
}
