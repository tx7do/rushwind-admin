//! The notification-domain wire enums: events, channels, deliveries
//! and the outbound provider settings.

enum_map!(notification_event_type_of + notification_event_type_str {
    "PASSWORD_RESET_CODE" => 1,
    "CONTACT_BIND_CODE" => 2,
    "CHANNEL_TEST_EMAIL" => 3,
    "INTERNAL_MESSAGE" => 4,
});

enum_map!(notification_channel_kind_of + notification_channel_kind_str {
    "EMAIL" => 1,
    "SMS" => 2,
    "WEBHOOK" => 3,
    "INTERNAL" => 4,
});

// 渠道表 type 列 ↔ proto Type 枚举（老三样 + 全部 EXT_* 扩展名，编码与
// notification_channel.proto 的 enum Type 逐字一致；EXT 名称即 ent 列值）。
enum_map!(notification_channel_type_of + notification_channel_type_str {
    "EMAIL" => 0,
    "WEBHOOK" => 1,
    "SMS" => 2,
    "WECOM_APP" => 100,
    "DINGTALK_APP" => 101,
    "FEISHU_APP" => 102,
    "WECHAT" => 103,
    "TELEGRAM" => 104,
    "SLACK" => 105,
    "DISCORD" => 106,
    "TEAMS" => 107,
    "GCHAT" => 108,
    "WEBEX" => 109,
    "LINE" => 110,
    "MATRIX" => 111,
    "MATTERMOST" => 112,
    "ROCKETCHAT" => 113,
    "ZULIP" => 114,
    "BARK" => 120,
    "NTFY" => 121,
    "GOTIFY" => 122,
    "SERVERCHAN" => 123,
    "WXPUSHER" => 124,
    "PUSHOVER" => 125,
    "PUSHBULLET" => 126,
    "PUSHDEER" => 127,
    "JPUSH" => 130,
    "GETUI" => 131,
    "HUAWEISMS" => 140,
    "YUNPIAN" => 141,
    "TWILIO" => 142,
    "VONAGE" => 143,
    "WHATSAPP" => 144,
    "MAILGUN" => 150,
    "SENDGRID" => 151,
    "POSTMARK" => 152,
    "IFTTT" => 160,
    "JOIN" => 161,
    "HOMEASSISTANT" => 162,
    "DATADOG" => 170,
    "OPSGENIE" => 171,
    "PAGERDUTY" => 172,
});

enum_map!(notification_delivery_status_of {
    "SENT" => 2,
    "FAILED" => 3,
    "SKIPPED" => 4,
});

enum_map!(notification_smtp_tls_of + notification_smtp_tls_str {
    "NONE" => 0,
    "SSL" => 2,
});

enum_map!(notification_sign_style_of + notification_sign_style_str {
    "NONE" => 1,
    "DINGTALK" => 2,
    "FEISHU" => 3,
    "WECOM" => 4,
});

enum_map!(notification_sms_provider_of + notification_sms_provider_str {
    "ALIYUN" => 0,
    "TENCENT" => 1,
});
