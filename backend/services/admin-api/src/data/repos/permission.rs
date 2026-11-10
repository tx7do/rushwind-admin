//! PermissionRepo — platform-global rows with
//! all predicates owned here (never ad-hoc in services).

use sea_orm::{EntityTrait, QueryFilter};

use crate::data::sys_permissions as entity;

repo_shell!(global PermissionRepo, entity, "permission");
