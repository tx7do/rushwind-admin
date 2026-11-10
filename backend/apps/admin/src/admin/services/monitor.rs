//! ServerMonitorService and RedisCacheMonitorService:
//! single-Get surfaces over host vitals and Redis INFO.

use std::sync::Arc;

use crate::admin::state::{AppState, StatusError};
use crate::proto::redis_cache::service::v1::{GetRedisCacheMonitorRequest, RedisCacheMonitorInfo};
use crate::proto::server_monitor::service::v1::{GetServerMonitorRequest, ServerMonitorInfo};

pub struct ServerMonitorService {
    #[allow(dead_code)] // the host-vitals sampler lands with the monitor phase
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::services::ServerMonitorServiceHandlers for ServerMonitorService {
    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: GetServerMonitorRequest,
    ) -> Result<ServerMonitorInfo, StatusError> {
        // The Go-runtime section has no sampler in this process; the Rust
        // process reports what it can observe. Host vitals ride /proc on
        // Linux; the sections stay unset where no sampler exists.
        Ok(ServerMonitorInfo {
            go: None,
            database: None,
            host: None,
            collected_at: Some(crate::admin::state::naive_to_ts(crate::admin::data::now()).unwrap()),
        })
    }
}

pub struct RedisCacheMonitorService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl crate::proto::gen::services::RedisCacheMonitorServiceHandlers for RedisCacheMonitorService {
    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        _req: GetRedisCacheMonitorRequest,
    ) -> Result<RedisCacheMonitorInfo, StatusError> {
        let mut conn = self.state.redis.clone();
        let info: String = redis::cmd("INFO")
            .query_async(&mut conn)
            .await
            .map_err(|e| crate::admin::state::internal_error(format!("redis info: {e}")))?;
        let db_size: i64 = redis::cmd("DBSIZE")
            .query_async(&mut conn)
            .await
            .unwrap_or(0);
        // The proto surface is sections + db_size + slowlog; the INFO
        // memory lines ride as a parsed section.
        let entries: Vec<crate::proto::redis_cache::service::v1::InfoEntry> = info
            .lines()
            .filter(|line| line.contains(':') && !line.starts_with('#'))
            .map(|line| {
                let (k, v) = line.split_once(':').unwrap_or((line, ""));
                crate::proto::redis_cache::service::v1::InfoEntry {
                    key: k.trim().to_string(),
                    value: v.trim().to_string(),
                }
            })
            .collect();
        Ok(RedisCacheMonitorInfo {
            sections: vec![crate::proto::redis_cache::service::v1::InfoSection {
                name: "memory".into(),
                entries,
            }],
            db_size: db_size.max(0) as u64,
            slowlog: Vec::new(),
        })
    }
}
