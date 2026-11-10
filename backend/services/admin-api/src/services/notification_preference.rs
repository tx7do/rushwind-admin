//! NotificationPreferenceService — 用户通知偏好（自助服务）：读写的都是
//! 当前操作人自己的偏好，无管理面、无平台守卫。
//!
//! user_id 从操作人钉定，客户端传什么都不影响写入对象。从未配置过偏好时
//! GetMy 返回默认窗口（22:00 → 08:00，与 ent 列默认值一致），落库发生在
//! 第一次保存。

use std::sync::Arc;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use chrono::Timelike;

use crate::state::{operator_of, status_error, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::notification::service::v1::{
    ListMyNotificationCategoriesResponse, NotificationCategoryItem, NotificationPreference,
    UpdateNotificationPreferenceRequest,
};

/// 静音时段默认窗口：22:00 → 次日 08:00（分钟数自当日 00:00 起）。
const DEFAULT_QUIET_START_MINUTE: i32 = 1320;
const DEFAULT_QUIET_END_MINUTE: i32 = 480;

/// 0..1439 — 分钟数的合法区间上界。
const QUIET_MINUTE_UPPER_BOUND: i32 = 24 * 60 - 1;

/// 退订分类数上限（防滥用；广播比对本身是 O(n)）。
const MUTED_CATEGORY_CAP: usize = 100;

pub struct NotificationPreferenceService {
    pub state: Arc<AppState>,
}

fn pref_proto(r: crate::data::sys_notification_preferences::Model) -> NotificationPreference {
    let muted = r.muted_category_ids.and_then(|v| {
        v.as_array()
            .map(|arr| arr.iter().filter_map(|x| x.as_u64()).collect::<Vec<u64>>())
    });
    NotificationPreference {
        user_id: Some(r.user_id as u64),
        quiet_enabled: r.quiet_enabled,
        quiet_start_minute: r.quiet_start_minute,
        quiet_end_minute: r.quiet_end_minute,
        muted_category_ids: muted.unwrap_or_default(),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
    }
}

/// 校验静音窗口：分钟数在 0..1439 且启用时 start != end（start == end 的
/// 窗口宽为零，等价于不静音，按配置错误拒绝而不是默默不生效）。
/// start > end 是合法的跨零点窗口（如 22:00→08:00）。
fn validate_quiet_window(
    enabled: bool,
    start_minute: i32,
    end_minute: i32,
) -> Result<(), StatusError> {
    for m in [start_minute, end_minute] {
        if !(0..=QUIET_MINUTE_UPPER_BOUND).contains(&m) {
            return Err(status_error(
                "BAD_REQUEST",
                "quiet minutes must be within 0..1439",
            ));
        }
    }
    if enabled && start_minute == end_minute {
        return Err(status_error(
            "BAD_REQUEST",
            "quiet start and end must differ",
        ));
    }
    Ok(())
}

/// 出站投递阶段的 SSE 推送抑制判定件；随出站里程碑接入发送链路，
/// 现阶段由单测钉住窗口语义（与校验层一致）。
#[allow(dead_code)]
pub struct QuietWindow {
    pub enabled: bool,
    pub start_minute: i32,
    pub end_minute: i32,
}

/// 判定 now 是否落在偏好定义的静音窗口内（服务器本地时间）。
/// 窗口跨零点（start > end）按"晚于 start 或早于 end"判定；
/// start == end 视为零宽窗口不静音（服务端兜底，与校验层一致）。
#[allow(dead_code)]
pub fn in_quiet_window(pref: &QuietWindow, now: chrono::NaiveTime) -> bool {
    if !pref.enabled {
        return false;
    }
    let minutes_of_day = (now.hour() * 60 + now.minute()) as i32;
    let (start, end) = (pref.start_minute, pref.end_minute);
    if start == end {
        return false;
    }
    if start < end {
        minutes_of_day >= start && minutes_of_day < end
    } else {
        minutes_of_day >= start || minutes_of_day < end
    }
}

#[async_trait::async_trait]
impl proto::gen::services::NotificationPreferenceServiceHandlers for NotificationPreferenceService {
    /// 返回当前用户的偏好；从未配置过时返回默认值（未启用静音、无退订），
    /// 前端据此渲染表单，落库发生在第一次保存。
    async fn get_my_notification_preference(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<NotificationPreference, StatusError> {
        let payload = operator_of(&ctx)?;
        let repo = crate::data::repos::NotificationPreferenceRepo::new(&self.state.db);
        match repo.get_by_user_id(payload.user_id).await? {
            Some(row) => Ok(pref_proto(row)),
            None => Ok(NotificationPreference {
                user_id: Some(payload.user_id as u64),
                quiet_enabled: Some(false),
                quiet_start_minute: Some(DEFAULT_QUIET_START_MINUTE),
                quiet_end_minute: Some(DEFAULT_QUIET_END_MINUTE),
                muted_category_ids: Vec::new(),
                updated_at: None,
            }),
        }
    }

    /// 保存当前用户的偏好（字段全量提交），返回保存后的完整状态。
    async fn update_my_notification_preference(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateNotificationPreferenceRequest,
    ) -> Result<NotificationPreference, StatusError> {
        let payload = operator_of(&ctx)?;
        validate_quiet_window(
            req.quiet_enabled.unwrap_or(false),
            req.quiet_start_minute.unwrap_or(DEFAULT_QUIET_START_MINUTE),
            req.quiet_end_minute.unwrap_or(DEFAULT_QUIET_END_MINUTE),
        )?;
        if req.muted_category_ids.len() > MUTED_CATEGORY_CAP {
            return Err(status_error("BAD_REQUEST", "too many muted categories"));
        }

        let muted = serde_json::to_value(&req.muted_category_ids)
            .map_err(|_| status_error("BAD_REQUEST", "invalid muted category ids"))?;

        let repo = crate::data::repos::NotificationPreferenceRepo::new(&self.state.db);
        let row = repo
            .upsert(
                payload.user_id,
                req.quiet_enabled.unwrap_or(false),
                req.quiet_start_minute.unwrap_or(DEFAULT_QUIET_START_MINUTE),
                req.quiet_end_minute.unwrap_or(DEFAULT_QUIET_END_MINUTE),
                Some(muted),
                payload.user_id,
            )
            .await?;
        Ok(pref_proto(row))
    }

    /// 当前用户可退订的启用分类清单（本租户内）。
    async fn list_my_notification_categories(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        _req: Empty,
    ) -> Result<ListMyNotificationCategoriesResponse, StatusError> {
        let scope = crate::data::Viewer::from_ctx(&ctx).tenant_scope();
        let mut query = crate::data::internal_message_categories::Entity::find()
            .filter(crate::data::internal_message_categories::Column::IsEnabled.eq(true))
            .order_by_asc(crate::data::internal_message_categories::Column::SortOrder);
        if let Some(tid) = scope {
            query =
                query.filter(crate::data::internal_message_categories::Column::TenantId.eq(tid));
        }
        let rows = query
            .all(&self.state.db)
            .await
            .map_err(|_| crate::state::internal_error("list categories failed"))?;
        Ok(ListMyNotificationCategoriesResponse {
            items: rows
                .into_iter()
                .map(|c| NotificationCategoryItem {
                    id: Some(c.id as u64),
                    name: Some(c.name),
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_window_bounds_and_zero_width() {
        assert!(validate_quiet_window(true, 0, 1439).is_ok());
        assert!(validate_quiet_window(true, 1320, 480).is_ok()); // 跨零点合法
        assert!(validate_quiet_window(false, 1320, 1320).is_ok()); // 未启用时零宽不拒
        assert!(validate_quiet_window(true, 1320, 1320).is_err()); // 启用时零宽拒绝
        assert!(validate_quiet_window(true, -1, 480).is_err());
        assert!(validate_quiet_window(true, 0, 1440).is_err());
    }

    #[test]
    fn quiet_window_crossing_and_zero_width_semantics() {
        let w = |s: i32, e: i32| QuietWindow {
            enabled: true,
            start_minute: s,
            end_minute: e,
        };
        let t = |h: u32, m: u32| chrono::NaiveTime::from_hms_opt(h, m, 0).unwrap();
        // 普通窗口 [08:00, 22:00)
        assert!(!in_quiet_window(&w(480, 1320), t(7, 59)));
        assert!(in_quiet_window(&w(480, 1320), t(8, 0)));
        assert!(in_quiet_window(&w(480, 1320), t(21, 59)));
        assert!(!in_quiet_window(&w(480, 1320), t(22, 0)));
        // 跨零点窗口 22:00→08:00 覆盖 [22:00, 24h) ∪ [00:00, 08:00)
        assert!(in_quiet_window(&w(1320, 480), t(23, 0)));
        assert!(in_quiet_window(&w(1320, 480), t(0, 0)));
        assert!(in_quiet_window(&w(1320, 480), t(7, 59)));
        assert!(!in_quiet_window(&w(1320, 480), t(8, 0)));
        assert!(!in_quiet_window(&w(1320, 480), t(12, 0)));
        // 零宽窗口不静音（服务端兜底）
        assert!(!in_quiet_window(&w(600, 600), t(6, 0)));
        // 未启用不静音
        let off = QuietWindow {
            enabled: false,
            start_minute: 1320,
            end_minute: 480,
        };
        assert!(!in_quiet_window(&off, t(23, 0)));
    }
}
