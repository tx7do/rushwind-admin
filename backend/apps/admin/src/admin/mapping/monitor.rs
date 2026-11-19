//! The monitor wire enums: the metric vocabulary and alert comparison
//! operators, shared by the `monitor_alert` and `server_monitor` proto
//! domains.

enum_map!(monitor_metric_of + monitor_metric_str => "monitor_alert.service.v1.MonitorMetric");

enum_map!(monitor_alert_op_of + monitor_alert_op_str => "monitor_alert.service.v1.AlertOp");
