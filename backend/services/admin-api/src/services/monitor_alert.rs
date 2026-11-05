//! MonitorAlertService — 监控告警规则 CRUD + 手动评估轮。
//!
//! 「告警→恢复」成对通知语义（与上游 evaluateOnce 逐字同源）：越限发告警、
//! 回到阈值内发恢复，持续越限按 cooldown_minutes 重发。通知本身随出站投递
//! 里程碑接入——现阶段每条结论如实标注 notified=false 与原因，但 firing
//! 状态、last_value/last_alerted_at 照常回写，保证切换出站后冷却/恢复
//! 语义无缝生效。采集失败轮：跳过该来源规则且**不清 firing 状态**——
//! 采集抖动不是恢复。

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::mapping;
use crate::state::{db_err, operator_of, require_data, status_error, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::monitor_alert::service::v1::{
    evaluate_monitor_alerts_response::RuleOutcome, CreateMonitorAlertRuleRequest,
    DeleteMonitorAlertRuleRequest, EvaluateMonitorAlertsRequest, EvaluateMonitorAlertsResponse,
    GetMonitorAlertRuleRequest, ListMonitorAlertRuleResponse, MonitorAlertRule,
    UpdateMonitorAlertRuleRequest,
};
use proto::proto::pagination::PagingRequest;

use proto::proto::monitor_alert::service::v1::MonitorMetric as MetricEnum;

fn rule_proto(r: crate::data::sys_monitor_alert_rules::Model) -> MonitorAlertRule {
    MonitorAlertRule {
        id: Some(r.id),
        name: Some(r.name),
        metric: r.metric.as_deref().and_then(mapping::monitor_metric_of),
        op: r.op.as_deref().and_then(mapping::monitor_alert_op_of),
        threshold: r.threshold,
        cooldown_minutes: r.cooldown_minutes,
        channel: r
            .channel
            .as_deref()
            .and_then(mapping::notification_channel_kind_of),
        target: Some(r.target),
        is_enabled: r.is_enabled,
        remark: r.remark,
        last_firing: r.last_firing,
        last_value: r.last_value,
        last_alerted_at: r.last_alerted_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
    }
}

fn upsert_fields(
    a: &mut crate::data::sys_monitor_alert_rules::ActiveModel,
    data: &MonitorAlertRule,
) -> Result<(), StatusError> {
    if let Some(v) = &data.name {
        a.name = Set(v.clone());
    }
    if let Some(v) = data.metric {
        let s = mapping::monitor_metric_str(v)
            .ok_or_else(|| status_error("BAD_REQUEST", "unknown metric"))?;
        a.metric = Set(Some(s.into()));
    }
    if let Some(v) = data.op {
        let s = mapping::monitor_alert_op_str(v)
            .ok_or_else(|| status_error("BAD_REQUEST", "unknown op"))?;
        a.op = Set(Some(s.into()));
    }
    if let Some(v) = data.threshold {
        a.threshold = Set(Some(v));
    }
    if let Some(v) = data.cooldown_minutes {
        if v > 1440 {
            return Err(status_error(
                "BAD_REQUEST",
                "cooldown_minutes must be within 0..1440",
            ));
        }
        a.cooldown_minutes = Set(Some(v));
    }
    if let Some(v) = data.channel {
        let s = mapping::notification_channel_kind_str(v)
            .ok_or_else(|| status_error("BAD_REQUEST", "unknown channel"))?;
        a.channel = Set(Some(s.into()));
    }
    if let Some(v) = &data.target {
        if v.len() < 3 || v.len() > 500 {
            return Err(status_error(
                "BAD_REQUEST",
                "target must be within 3..500 chars",
            ));
        }
        a.target = Set(v.clone());
    }
    if let Some(v) = data.is_enabled {
        a.is_enabled = Set(Some(v));
    }
    if let Some(v) = &data.remark {
        a.remark = Set(Some(v.clone()));
    }
    Ok(())
}

/// 校验规则：非布尔指标必须带 op/threshold；渠道/目标必配。
fn validate_alert_rule(data: &MonitorAlertRule) -> Result<(), StatusError> {
    let metric = data
        .metric
        .and_then(mapping::monitor_metric_str)
        .unwrap_or_default();
    if metric != "DB_PING_FAIL" && (data.op.is_none() || data.threshold.is_none()) {
        return Err(status_error(
            "BAD_REQUEST",
            "op and threshold are required for non-boolean metrics",
        ));
    }
    if data.target.as_deref().map_or(true, |t| t.trim().is_empty()) {
        return Err(status_error("BAD_REQUEST", "target is required"));
    }
    Ok(())
}

pub struct MonitorAlertService {
    pub state: Arc<AppState>,
}

/// 一轮指标采样（server 侧三个 GO_/DB_ 指标共享一次读取）。
struct ServerSample {
    goroutines: Option<f64>,
    mem_alloc_mb: Option<f64>,
    db_open_connections: Option<f64>,
    db_ping_fail: Option<f64>,
}

impl MonitorAlertService {
    /// server 侧采样：线程数/RSS 代理 Go 运行时两指标（本进程是 Rust
    /// 运行时，线程与 RSS 是同语义的进程观测），连接数读 pg_stat_activity，
    /// ping 走 SELECT 1。任一读取失败记 None → 该指标规则本轮跳过。
    async fn collect_server(&self) -> ServerSample {
        use sea_orm::ConnectionTrait;
        // 线程数与 RSS：/proc/self/status（Linux）；无该接口的宿主留 None。
        let (threads, rss_mb) = read_proc_self_status();
        let db_open = self
            .state
            .db
            .query_one_raw(sea_orm::Statement::from_string(
                self.state.db.get_database_backend(),
                "SELECT COUNT(*) AS cnt FROM pg_stat_activity WHERE datname = current_database()",
            ))
            .await
            .ok()
            .flatten()
            .and_then(|row| row.try_get::<i64>("", "cnt").ok())
            .map(|c| c as f64);
        let ping_ok = self
            .state
            .db
            .query_one_raw(sea_orm::Statement::from_string(
                self.state.db.get_database_backend(),
                "SELECT 1",
            ))
            .await;
        ServerSample {
            goroutines: threads,
            mem_alloc_mb: rss_mb,
            db_open_connections: db_open,
            db_ping_fail: ping_ok.is_err().then_some(1.0),
        }
    }

    /// redis 侧采样：DBSIZE。
    async fn collect_redis(&self) -> Option<f64> {
        let mut conn = self.state.redis.clone();
        redis::cmd("DBSIZE")
            .query_async::<i64>(&mut conn)
            .await
            .ok()
            .map(|n| n as f64)
    }

    /// 单规则指标取值：来源采样缺失 → None（采集失败轮）。
    fn metric_value(metric: i32, server: &ServerSample, redis_size: Option<f64>) -> Option<f64> {
        match MetricEnum::try_from(metric) {
            Ok(MetricEnum::GoGoroutines) => server.goroutines,
            Ok(MetricEnum::GoMemAllocMb) => server.mem_alloc_mb,
            Ok(MetricEnum::DbOpenConnections) => server.db_open_connections,
            Ok(MetricEnum::DbPingFail) => server.db_ping_fail,
            Ok(MetricEnum::RedisDbSize) => redis_size,
            _ => None,
        }
    }

    /// 阈值判定。DB_PING_FAIL 是布尔指标、绕过比较：ping 失败（值 1）即越限，
    /// 成功（0）即恢复；其余按 GE/LE。
    fn is_breached(metric: i32, op: Option<&str>, threshold: Option<f64>, value: f64) -> bool {
        if metric == MetricEnum::DbPingFail as i32 {
            return value >= 1.0;
        }
        match op {
            Some("GE") => value >= threshold.unwrap_or_default(),
            Some("LE") => value <= threshold.unwrap_or_default(),
            _ => false,
        }
    }
}

/// (threads, rss_mb) from /proc/self/status；非 Linux 宿主 (None, None)。
fn read_proc_self_status() -> (Option<f64>, Option<f64>) {
    let Ok(text) = std::fs::read_to_string("/proc/self/status") else {
        return (None, None);
    };
    let mut threads = None;
    let mut rss_mb = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("Threads:") {
            threads = v.trim().parse::<f64>().ok();
        } else if let Some(v) = line.strip_prefix("VmRSS:") {
            // "  123456 kB"
            if let Some(kb) = v.split_whitespace().next() {
                rss_mb = kb.parse::<f64>().ok().map(|k| k / 1024.0);
            }
        }
    }
    (threads, rss_mb)
}

/// 冷却判定：持续越限重发以 last_alerted_at 为锚。
fn cooldown_elapsed(last_alerted_at: Option<chrono::NaiveDateTime>, minutes: u32) -> bool {
    match last_alerted_at {
        None => true,
        Some(t) => {
            let elapsed = (crate::data::now() - t).num_seconds().max(0);
            elapsed >= i64::from(minutes) * 60
        }
    }
}

#[async_trait::async_trait]
impl proto::gen::services::MonitorAlertServiceHandlers for MonitorAlertService {
    async fn list_monitor_alert_rule(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListMonitorAlertRuleResponse, StatusError> {
        let repo = crate::data::repos::MonitorAlertRepo::new(&self.state.db);
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListMonitorAlertRuleResponse {
            items: rows.into_iter().map(rule_proto).collect(),
            total,
        })
    }

    async fn get_monitor_alert_rule(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetMonitorAlertRuleRequest,
    ) -> Result<MonitorAlertRule, StatusError> {
        let repo = crate::data::repos::MonitorAlertRepo::new(&self.state.db);
        Ok(rule_proto(repo.get_by_id(req.id).await?))
    }

    async fn create_monitor_alert_rule(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateMonitorAlertRuleRequest,
    ) -> Result<MonitorAlertRule, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        validate_alert_rule(&data)?;
        let mut a = crate::data::sys_monitor_alert_rules::ActiveModel {
            name: Set(data.name.clone().unwrap_or_default()),
            target: Set(data.target.clone().unwrap_or_default()),
            cooldown_minutes: Set(Some(data.cooldown_minutes.unwrap_or(30))),
            last_firing: Set(Some(false)),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        };
        upsert_fields(&mut a, &data)?;
        let inserted = a.insert(&self.state.db).await.map_err(db_err)?;
        Ok(rule_proto(inserted))
    }

    async fn update_monitor_alert_rule(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateMonitorAlertRuleRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = require_data(req.data)?;
        validate_alert_rule(&data)?;
        let repo = crate::data::repos::MonitorAlertRepo::new(&self.state.db);
        let row = repo.get_by_id(req.id).await?;
        let mut a: crate::data::sys_monitor_alert_rules::ActiveModel = row.into();
        upsert_fields(&mut a, &data)?;
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete_monitor_alert_rule(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteMonitorAlertRuleRequest,
    ) -> Result<Empty, StatusError> {
        let repo = crate::data::repos::MonitorAlertRepo::new(&self.state.db);
        repo.get_by_id(req.id).await?;
        repo.delete_by_id(req.id).await?;
        Ok(Empty {})
    }

    /// 立即评估一轮（与周期扫描同一内核）：采样一次 → 逐条启用规则求值 →
    /// 状态回写；通知随出站里程碑，notified 恒 false 且带原因。
    async fn evaluate_monitor_alerts(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: EvaluateMonitorAlertsRequest,
    ) -> Result<EvaluateMonitorAlertsResponse, StatusError> {
        let repo = crate::data::repos::MonitorAlertRepo::new(&self.state.db);
        let rules = repo.list_enabled().await?;
        let server = self.collect_server().await;
        let redis_size = self.collect_redis().await;

        let mut outcomes = Vec::with_capacity(rules.len());
        for rule in rules {
            let metric = rule
                .metric
                .as_deref()
                .and_then(mapping::monitor_metric_of)
                .unwrap_or(MetricEnum::GoGoroutines as i32);
            let mut a: crate::data::sys_monitor_alert_rules::ActiveModel = rule.clone().into();

            let Some(value) = Self::metric_value(metric, &server, redis_size) else {
                outcomes.push(RuleOutcome {
                    rule_id: rule.id,
                    name: rule.name.clone(),
                    firing: rule.last_firing.unwrap_or(false),
                    current_value: None,
                    notified: false,
                    // 采集失败轮：跳过且不清 firing 状态（抖动不是恢复）。
                    reason: Some("采集失败".into()),
                });
                continue;
            };

            let breached = Self::is_breached(metric, rule.op.as_deref(), rule.threshold, value);
            let was_firing = rule.last_firing.unwrap_or(false);
            // 通知本身随出站投递里程碑接入；notified 恒 false 但语义分类
            // 照常给出（告警/重发/恢复/冷却中/未越限未恢复）。
            let notified = false;
            let (reason, alert_now) = if breached && !was_firing {
                // 首次触发 → 告警通知
                (Some("越限告警待发：Rust 侧出站未移植".to_string()), true)
            } else if breached && was_firing {
                if cooldown_elapsed(rule.last_alerted_at, rule.cooldown_minutes.unwrap_or(30)) {
                    (
                        Some("冷却到期重发待发：Rust 侧出站未移植".to_string()),
                        true,
                    )
                } else {
                    (Some("冷却中".to_string()), false)
                }
            } else if !breached && was_firing {
                // 恢复通知（不等冷却：恢复通知晚到没有意义）
                (Some("恢复通知待发：Rust 侧出站未移植".to_string()), true)
            } else {
                (Some("未越限未恢复".to_string()), false)
            };
            if alert_now {
                a.last_alerted_at = Set(Some(crate::data::now()));
            }

            a.last_firing = Set(Some(breached));
            a.last_value = Set(Some(value));
            a.updated_at = Set(Some(crate::data::now()));
            let _ = a.update(&self.state.db).await.map_err(db_err)?;

            outcomes.push(RuleOutcome {
                rule_id: rule.id,
                name: rule.name.clone(),
                firing: breached,
                current_value: Some(value),
                notified,
                reason,
            });
        }

        Ok(EvaluateMonitorAlertsResponse { outcomes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_metric_bypasses_threshold_compare() {
        let ping_fail = MetricEnum::DbPingFail as i32;
        let redis_size = MetricEnum::RedisDbSize as i32;
        // DB_PING_FAIL：值 1 即越限，0 即恢复，op/threshold 被忽略
        assert!(MonitorAlertService::is_breached(ping_fail, None, None, 1.0));
        assert!(!MonitorAlertService::is_breached(
            ping_fail, None, None, 0.0
        ));
        // 数值指标按 GE/LE
        assert!(MonitorAlertService::is_breached(
            redis_size,
            Some("GE"),
            Some(100.0),
            101.0
        ));
        assert!(!MonitorAlertService::is_breached(
            redis_size,
            Some("GE"),
            Some(100.0),
            99.0
        ));
        assert!(MonitorAlertService::is_breached(
            redis_size,
            Some("LE"),
            Some(10.0),
            9.0
        ));
    }

    #[test]
    fn cooldown_anchors_on_last_alerted_at() {
        assert!(cooldown_elapsed(None, 30));
        let now = crate::data::now();
        // 31 分钟前 → 到期
        let old = now - chrono::Duration::minutes(31);
        assert!(cooldown_elapsed(Some(old), 30));
        // 29 分钟前 → 冷却中
        let recent = now - chrono::Duration::minutes(29);
        assert!(!cooldown_elapsed(Some(recent), 30));
    }

    #[test]
    fn proc_status_reader_parses_or_yields_none() {
        let (threads, rss) = read_proc_self_status();
        // Linux 测试宿主上应解析出正数；其他宿主两者皆 None
        match (threads, rss) {
            (Some(t), Some(m)) => {
                assert!(t > 0.0 && m >= 0.0);
            }
            (None, None) => {}
            other => panic!("mixed sample {other:?}"),
        }
    }
}
