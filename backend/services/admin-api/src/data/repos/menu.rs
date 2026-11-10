//! MenuRepo — platform-global
//! rows (menus carry no tenant column), all predicates owned here.

use sea_orm::{EntityTrait, QueryFilter};

use crate::data::sys_menus as entity;

repo_shell!(global MenuRepo, entity, "menu");
