//! The notification-domain wire enums: events, channels, deliveries
//! and the outbound provider settings.

enum_map!(notification_event_type_of + notification_event_type_str => "notification.service.v1.EventType");

enum_map!(notification_channel_kind_of + notification_channel_kind_str => "notification.service.v1.Channel");

// 渠道表 type 列 ↔ proto Type 枚举（老三样 + 全部 EXT_* 扩展名，EXT 名称即
// ent 列值，编码由描述符池按 notification_channel.proto 的 enum Type 给出）。
enum_map!(notification_channel_type_of + notification_channel_type_str => "notification_channel.service.v1.NotificationChannel.Type");

enum_map!(notification_delivery_status_of => "notification.service.v1.DeliveryStatus");

enum_map!(notification_smtp_tls_of + notification_smtp_tls_str => "notification_channel.service.v1.NotificationChannel.TlsMode");

enum_map!(notification_sign_style_of + notification_sign_style_str => "notification_channel.service.v1.SignStyle");

enum_map!(notification_sms_provider_of + notification_sms_provider_str => "notification_channel.service.v1.SmsProvider");
