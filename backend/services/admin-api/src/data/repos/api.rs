//! ApiRepo — platform-global rows with
//! all predicates owned here (never ad-hoc in services).

use sea_orm::{EntityTrait, QueryFilter};

use crate::data::sys_apis as entity;

repo_shell!(global ApiRepo, entity, "api");
