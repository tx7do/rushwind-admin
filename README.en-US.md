<div align="center">

<img src="docs/brand/rushwind-icon.svg" alt="RushWind Admin" width="128">

# RushWind Admin

[![License](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.81+-DEA584?logo=rust)](https://www.rust-lang.org/)
[![CI](https://github.com/tx7do/rushwind-admin/actions/workflows/ci.yml/badge.svg)](https://github.com/tx7do/rushwind-admin/actions/workflows/ci.yml)

**English** | [中文](./README.md) | [日本語](./README.ja-JP.md)

</div>

---

## Highlights

- **Multi-frontend**: ships with three frontends — `Vue3 Vben` (Ant Design Vue), `Vue3 Element Plus`, and `React19 Antd` — so teams can pick their flavor
- **Enterprise-grade RBAC**: multi-tenant, multi-role, multi-department, with menu/button/data-level permission control
- **Contract-driven code generation**: Protobuf is the single API contract — 211 routes, 441 error-status mappings, and 206 service interfaces are deterministically generated at build time with zero hand-written routing; the TypeScript clients for the three frontends are byte-identical
- **Security & MLPS 2.0 compliance**: 180-day audit log retention & archiving, password policy suite, TOTP MFA, application-layer password encryption, dynamic RBAC and multi-tenant isolation — see [Security & Compliance](#security--mlps-20-compliance)
- **Production-ready foundation**: JWT RS256 auth, unified four-field error envelope, CORS, SSE push (planned), async task scheduling (planned)
- **Quality gates**: four CI gates (fmt / clippy / test / contract sync) on an ubuntu + windows matrix, plus a differential test rig that sweeps and byte-compares all routes automatically

## Demo

| Frontend | Demo |
|---------|--------|
| Vue3 Vben | <https://vben.admin.gowind.cloud> |
| Vue3 Element Plus | <https://ele.admin.gowind.cloud> |
| React | <https://react.admin.gowind.cloud> |

- Backend Swagger: <https://api.demo.admin.gowind.cloud/docs/>
- Default account: `admin` / `Abcd@1234`

---

## Screenshots

### Admin console

All three frontends talk to this repository's backend without a single code change; the shots below come from the React build against REST :7788:

**Sign-in** — image captcha plus a tenant code (leave it blank for platform sign-in); passwords are encrypted at the application layer

![Sign-in](./docs/screenshots/console-login.png)

**Dashboard** — live counts for users, roles, logins and audit entries, with trend and share charts

![Dashboard](./docs/screenshots/console-dashboard.png)

**Users** — organization tree with advanced filters; multi-role, multi-department and status management

![Users](./docs/screenshots/console-users.png)

**Menus** — directory / menu / button nodes, with permission tags bound to route and component paths

![Menus](./docs/screenshots/console-menus.png)

**Operation audit log** — action type, target resource and request ID recorded for every success and failure

![Operation audit log](./docs/screenshots/console-op-audit.png)

**Online sessions** — session and device view with force-logout

![Online sessions](./docs/screenshots/console-online.png)

### API documentation

The backend serves its own API documentation (the `enable_swagger` / `enable_redoc` switches in `server.yaml`; the raw spec is at `/q/openapi.yaml`) — no separate docs site to deploy.

**Swagger UI** — `/q/swagger-ui`: every endpoint grouped by service, with an authorize entry and a try-it-out console

![Swagger UI](./docs/screenshots/swagger-ui.png)

**ReDoc** — `/q/redoc`: a three-pane reference that puts parameter docs beside request / response samples

![ReDoc](./docs/screenshots/redoc.png)

---

## Tech Stack

<table>
<tr><th>Layer</th><th>Technology</th></tr>
<tr><td><strong>Backend framework</strong></td><td><code>Rust</code> (edition 2021 / MSRV 1.81) · <code>RushWind</code> framework · <code>axum 0.8</code> · <code>tokio</code></td></tr>
<tr><td><strong>Contract pipeline</strong></td><td><code>Protobuf</code> · <code>buf + protox</code> · <code>prost / prost-reflect / pbjson</code> · <code>rushwind-gen-http</code> (route generation)</td></tr>
<tr><td><strong>Storage</strong></td><td><code>SeaORM</code> · <code>PostgreSQL</code> · <code>Redis</code> (in progress)</td></tr>
<tr><td><strong>Auth</strong></td><td><code>JWT RS256</code> (rushwind-authn-jwt) · <code>RBAC</code> (planned)</td></tr>
<tr><td><strong>Vue Vben edition</strong></td><td><code>Vue 3</code> · <code>TypeScript</code> · <code>Vite</code> · <code>Ant Design Vue</code> · <code>Vben Admin</code></td></tr>
<tr><td><strong>Vue Element edition</strong></td><td><code>Vue 3</code> · <code>TypeScript</code> · <code>Vite</code> · <code>Element Plus</code> (lightweight)</td></tr>
<tr><td><strong>React edition</strong></td><td><code>React 19</code> · <code>TypeScript</code> · <code>Vite</code> · <code>Zustand</code> · <code>Ant Design V6</code> (no UMI)</td></tr>
<tr><td><strong>Quality gates</strong></td><td><code>Differential test rig</code> · <code>cargo fmt / clippy / test</code> · <code>GitHub Actions</code> (ubuntu + windows matrix)</td></tr>
</table>

---

## Security & MLPS 2.0 Compliance

The security capabilities of this project are designed against the technical requirements of China's MLPS 2.0 (Level 2/3), ready out of the box for enterprise private deployments with strict privacy demands:

| MLPS requirement | Implementation |
|------------|---------|
| **Security audit** | Six audit log categories: login / operation / API / data access / permission change / policy evaluation, with IP geolocation and trace_id; retention and archiving policies are tunable (180 days in-database by default, expired data exported as JSONL archives) |
| **Identity authentication** | Password complexity (≥8 chars, 3-of-4 character classes), password history reuse check (last 3 by default), password expiry (90 days by default) — thresholds adjustable via the "parameter management" console; TOTP MFA; captcha; login rate limiting (IP + username); configurable login restriction policies |
| **Access control** | Dynamic RBAC engine with role–permission–API mappings stored in the database and hot-reloaded on change; menu/button-level permissions; role-scoped row-level data ranges and field-level permissions (blacklisted fields trimmed from responses); every authorization decision recorded in policy evaluation logs |
| **Multi-tenant isolation** | Storage-layer data isolation: tenant filters injected into reads automatically, writes guarded against tenant forgery, updates/deletes forced through tenant predicates; tenant requests validated fail-closed against the Api table by `(path, method)`; plan module whitelists and read-only expiry policy |
| **Data confidentiality** | Application-layer AES encryption for passwords in transit, bcrypt hashing at rest; JWT RS256 asymmetric signing; refresh token in an HttpOnly cookie; transport TLS terminated at the deployment layer |
| **Backup & recovery** | Scheduled full backups (pg_dump, 30 copies retained with automatic rotation), supporting both Docker container and local direct-connection modes |
| **Frontend security** | All three frontends ship production builds with CSP, X-Frame-Options, HSTS, and related security headers |

> **Note**: MLPS assessment covers non-software aspects too (management systems, physical environment, staffing). This project addresses the technical measures; it supports but does not replace a full MLPS assessment process.

---

## Current Status

The project advances through the phases defined in [docs/development-plan.md](./docs/development-plan.md). The contract surface is complete; service implementations are landing module by module.

**Landed**

- Contract pipeline: proto sync (MANIFEST gate + drift detection) → annotated descriptor → build-time deterministic generation of routes / error-status table / service interfaces / mount glue
- REST :7788 assembly: 211 routes, an 8-endpoint auth-free whitelist, JWT RS256 auth gate, CORS, unified four-field error envelope (code / reason / message / metadata)
- Codec alignment: protojson request binding and response emission (64-bit integers as strings, presence omission, well-known type semantics), pinned by golden tests
- Differential test rig: automated sweep of 211 routes + 93 HEAD probes, 4 exemption classes explicitly registered
- Real service implementations for part of the system modules (users / roles / tenants / dictionaries / authentication / MFA, etc.); the remaining endpoints return Unknown stubs

**In progress / planned**

- Storage layer: RBAC policy loading, session-revocation checker, query alias layer
- Replacing the remaining stub services module by module
- SSE (:7789), async task queue, audit log archiving, scripting system

---

## Quick Start

### Requirements

| Tool | Version |
|------|------|
| Rust | stable (workspace `rust-version = 1.81`) |
| buf | latest (`curl -fsSL https://buf.build/install.sh | sh`, or a single binary from [GitHub releases](https://github.com/bufbuild/buf/releases)); must be on PATH for `cargo build` (annotated closure) |
| bash | for sync / rig scripts (Git Bash recommended on Windows) |
| Docker | 20.0+ (local middleware / differential test rig) |
| Node.js + pnpm | per each frontend's `package.json` `engines` (current intersection ≥ 20.19.0), pnpm >= 10.0.0 |

### Sibling Clone Layout

This repository references its sibling repositories via relative paths, so clone them **under the same parent directory**:

```text
<parent>/
├── rushwind-admin/   # this repo
├── rushwind/         # RushWind framework monorepo (required)
└── rust-utils/       # utility library (required)
```

### Backend Startup

```shell
cd backend
cargo run -p admin-api   # binary admin-api, serving REST :7788
```

- On startup it connects to PostgreSQL and Redis. Configuration lives in `backend/services/admin-api/assets/` (`auth.yaml` / `data.yaml` / `oss.yaml`)
- `assets/jwt_public_key.pem` and the key embedded in `auth.yaml` are **demo keys** — replace them before any production deployment
- SSE (:7789) is not available yet

### Contract Sync

The proto contract is synced from the upstream contract source by script, guarded by a MANIFEST checksum gate:

```shell
bash backend/api/sync-protos.sh          # sync protos and rebuild the MANIFEST
bash backend/api/sync-protos.sh --check  # verification gate (same as CI)
```

See the script header (`backend/api/sync-protos.sh`) for the default source path and its override. **Never edit** `backend/api/protos/` by hand.

### Quality Gates

```shell
cd backend
cargo fmt -p proto -p auth -p admin-api -p admin-diff -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

CI (ubuntu + windows matrix) runs the same four gates: fmt / clippy / test / contract sync — see [.github/workflows/ci.yml](./.github/workflows/ci.yml).

### Differential Test Rig

[backend/testbed](./backend/testbed) brings up the middleware plus the Go and Rust backends via compose; `admin-diff` sweeps 211 routes + 93 HEAD probes automatically and compares responses byte-for-byte, with exemptions registered in `exemptions.json`. See [backend/testbed/README.md](./backend/testbed/README.md) for usage.

### Frontend Startup

The frontends and the backend form a zero-change compatibility contract: point the API base URL at this backend (REST :7788) — no frontend code changes required. All three editions ship with this repo as synced snapshots (with a RushWind brand overlay on top, guarded by dual-manifest anti-tamper gates), and their dev proxies already target :7788:

| Frontend | Directory | Status | Command | Port |
|---------|------|------|---------|------|
| React | `frontend/admin/react` | ✅ in-repo snapshot + brand overlay | `pnpm dev` | 5888 |
| Vue Element | `frontend/admin/vue-element` | ✅ in-repo snapshot + brand overlay | `pnpm dev` | 5777 |
| Vue Vben | `frontend/admin/vue-vben` | ✅ in-repo snapshot + brand overlay | `pnpm dev:antd` | 5666 |

```shell
# React edition (this repo, frontend/admin/react)
cd frontend/admin/react
pnpm install
pnpm dev            # :5888, proxied to REST :7788

# Vue Element edition (this repo, frontend/admin/vue-element)
cd frontend/admin/vue-element
pnpm install
pnpm dev            # :5777, proxied to REST :7788

# Vue Vben edition (this repo, frontend/admin/vue-vben, monorepo, run at its root)
cd frontend/admin/vue-vben
pnpm install
pnpm dev:antd       # :5666, proxied to REST :7788
```

---

## Features

> Every list page (business data and audit logs) supports paginated aggregate export with the current filters applied, as CSV / XLSX (up to 10k rows). For backend implementation coverage per module, see [Current Status](#current-status).

### Organization & Permissions

| Feature | Description |
|------|-----|
| User management | Manage and query users with advanced search and department-linked user listing; enable/disable users, set/unset supervisors, reset passwords, assign multiple roles, departments and supervisors, one-click login as a user, and more |
| Tenant management | Manage tenants; creating a tenant auto-initializes its department, default role and admin. Supports plan assignment, enable/disable, one-click login as the tenant admin |
| Plans & quotas | Manage tenant subscription plans and resource quotas (module whitelists, usage caps), with full CRUD for plans and quota items |
| Role management | Manage roles and role groups with role-linked user listing; menu authorization, row-level data ranges (five tiers / custom org-unit sets), field-level permissions (blacklisted field sets), batch employee add/remove |
| Permission management | Manage permission groups, menus and permission points in tree views |
| Organization management | Manage organizations in tree views |
| Position management | Manage user positions as user tags; Excel import (client template download, per-row creation via existing APIs, row-level error reporting, organization column matched by exact organization name) |
| Menu management | Configure system menus, operation permissions and button permission identifiers across directories, menus and buttons; menu sync (available on all three frontends, transactional rebuild or incremental merge; merge updates in place by full path, preserving existing menu IDs and role grants) |

### System Features

| Feature | Description |
|------|-----|
| API management | Manage APIs with sync support, mainly for selecting endpoints when creating permission points; tree views, operation-log request/response capture configuration |
| Dictionary management | Manage dictionary categories and entries with category-to-entry linking, server-side multi-column sorting, import and export |
| Task scheduling | Manage and inspect tasks and their run logs; create, update, delete, start, pause and run tasks immediately |
| File management | Manage uploads with query, upload to OSS or local storage, download, copy URL, deletion, and image preview |
| Login policy | Manage login restriction policies: target users, restriction type, mode, value and reason |
| Account login | Username / email / phone multi-identifier login, combinable with captcha, login policies and TOTP MFA |
| Multi-factor authentication (MFA) | TOTP-based MFA with login challenges, personal-center binding management, and an admin rescue path to reset a user's MFA |
| Password recovery | Email verification-code recovery: codes valid for 10 minutes and single use, all sessions revoked on success, silent handling to prevent user enumeration |
| Notification channels | Manage notification channels (EMAIL / SMTP) with encrypted passwords, masked list display, enable/disable and test sends |
| Service monitoring | Read-only runtime metrics (CPU cores, memory, uptime, etc.) with auto refresh |
| Scripting system | Script-level plugin system (Lua / JavaScript, database as source of truth, changes take effect immediately): entity lifecycle hooks (before can veto / after async), scheduled tasks, outbound HTTP (fail-closed domain whitelist), dry runs and execution logs |
| Parameter management | Key-value management of platform-wide system parameters (distinct from business dictionaries); built-in parameters seeded at startup, protected from deletion; read by services through a cached accessor; changes broadcast via Redis pub/sub to invalidate caches across instances |
| Machine credentials (AK/SK) | Tenant-level AccessKey / SecretKey management: one-time Secret display at creation, enable/disable, deletion and rotation (old Secret invalidated immediately); AK/Secret can be exchanged for a tenant-scoped machine JWT (machine role, access-token only) at a rate-limited endpoint (per IP + AK) |
| Language management | Manage supported languages: name, code, native name, enabled and default flags |

### Messaging & Logs

| Feature | Description |
|------|-----|
| Message categories | Manage message categories, two levels of custom categories, used by message management |
| Message management | Manage messages with per-scope sending (all users / selected users) and recall; all-user broadcasts delivered via the async task queue (resumable, idempotent); read receipts with timestamps |
| Internal messages | Manage in-app messages: view details, delete, mark as read, mark all as read |
| Login logs | Login log listing for successful and failed logins, with IP geolocation |
| Operation logs | Operation log listing for normal and exceptional actions, with IP geolocation and resource locating, plus detail views |
| API logs | API log listing with operator, request path, method and success status, with IP geolocation |
| Data logs | Data access log listing with lexically sanitized SQL and automatic extraction of involved tables and data classifications |
| Permission logs | Permission change log listing with operator, target object and reason, retaining request snapshots |
| Policy evaluation logs | Policy evaluation log listing with each authorization decision and its evaluation context, traceable via trace_id |
| Redis cache monitor | Read-only Redis INFO, DBSIZE and slow log viewer; no write operations |

### Personal Center

| Feature | Description |
|------|-----|
| Personal center | View and edit profile, last login info, password change, email binding / rebinding (verification-code checked), and more |

---

## Project Structure

```text
rushwind-admin/
├── backend/
│   ├── api/                        # API 契约（唯一契约源）
│   │   ├── protos/                 # proto 契约副本（MANIFEST.sha256 校验门）
│   │   ├── third_party/            # 第三方 proto（google.api 等）
│   │   └── sync-protos.sh          # 契约同步与校验脚本
│   ├── crates/                     # 共享 crate（proto 契约生成 crate、auth 鉴权门）
│   ├── services/
│   │   └── admin-api/              # Admin 服务 crate（src/ 模块树 + assets/ 内嵌资源）
│   └── testbed/                    # 差分回归台架（compose + admin-diff sweep）
├── frontend/                       # three synced frontend snapshots (sync-frontend.sh + dual manifests + RushWind brand overlay)
├── docs/                           # project docs (binding-spec / development-plan / operator-matrix / screenshots)
└── .github/workflows/              # CI (fmt / clippy / test / contract sync gates)
```

---

## Related Projects

- **[rushwind](https://github.com/tx7do/rushwind)** — the RushWind framework monorepo (http / http-binding / authn / transport-axum / gen-http, etc.)
- **[rust-utils](https://github.com/tx7do/rust-utils)** — Rust utility library (`query_parser`, etc.)

## Community & Contribution

Contributions to RushWind Admin are welcome:

- [Contributing Guide](./CONTRIBUTING.md) — sibling clone layout, contract sync conventions, commit conventions, and the PR process
- [Security Policy](./SECURITY.md) — vulnerability reporting process and scope

## Contact

- WeChat: `yang_lin_bo` (note: `rushwind-admin`)
- Juejin column: <https://juejin.cn/column/7541283508041826367>

## Acknowledgements

[![JetBrains](https://resources.jetbrains.com/storage/products/company/brand/logos/jb_beam.svg)](https://jb.gg/OpenSource)

Thanks to JetBrains for providing free open-source licenses.
