//! The monitor wire enums: the metric vocabulary and alert comparison
//! operators, shared by the `monitor_alert` and `server_monitor` proto
//! domains.

enum_map!(monitor_metric_of + monitor_metric_str {
    "GO_GOROUTINES" => 1,
    "GO_MEM_ALLOC_MB" => 2,
    "DB_OPEN_CONNECTIONS" => 3,
    "DB_PING_FAIL" => 4,
    "REDIS_DB_SIZE" => 5,
});

enum_map!(monitor_alert_op_of + monitor_alert_op_str {
    "GE" => 1,
    "LE" => 2,
});
