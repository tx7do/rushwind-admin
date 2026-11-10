//! NotificationChannelRepo — platform-global rows with
//! all predicates owned here (never ad-hoc in services).

use sea_orm::{EntityTrait, QueryFilter};

use crate::data::sys_notification_channels as entity;

repo_shell!(global NotificationChannelRepo, entity, "notification channel");
