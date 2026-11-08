//! 出站投递 sender 层（上游 data/channel 同构）：按渠道分发的"怎么发"，
//! sys_notification_channels 表是"用什么账号发"。
//!
//! 判据只有一条：渠道有没有"真的试过"。配置类错误（没配/没启用/类型不对/
//! 地址不合法）＝一条都没发出去 → [`Outcome::Skipped`]；其余（认证被拒、
//! 连接超时、对端 5xx）＝拨过号了 → [`Outcome::Failed`]。台账据此落
//! SKIPPED / FAILED，找回密码的回话文案就靠这个区分。
//!
//! 渠道选择策略收在 [`resolve_channel`] 一处：显式 channel_id 则用该条
//! （配置页的测试投递），否则取第一个启用的同类型渠道（系统发起的投递）。
//! EXT_* 扩展渠道是声明边界：schema 必填预检已过、投递面随独立里程碑接入。

use std::collections::BTreeMap;

use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

use crate::data::sys_notification_channels as chan;
use crate::state::StatusError;

/// 一次投递意图（不含模板/参数渲染——正文由调用方给出）。
pub struct SendRequest {
    /// 投递目标：EMAIL 收件地址 / WEBHOOK URL / SMS 手机号。
    pub target: String,
    pub title: String,
    pub content: String,
    /// 显式渠道配置 ID；0 = 由渠道策略自选（第一个启用的同类型渠道）。
    pub channel_id: u32,
    pub event_type: String,
    /// 渠道原生参数：SMS 只消费 params["code"]（服务商审核模板的验证码占位值）。
    pub params: BTreeMap<String, String>,
}

/// 投递结论：台账的写法由它决定（SENT / FAILED / SKIPPED）。
pub enum Outcome {
    Sent {
        channel_id: u32,
    },
    /// 拨过号了才走这里；channel_id 在账号已选出时带回（排障先知道走的哪条配置）。
    Failed {
        channel_id: Option<u32>,
        error: String,
    },
    /// 一次都没联系对端（配置类原因）。
    Skipped {
        error: String,
    },
}

impl Outcome {
    /// 台账 status 列的值。
    pub fn status(&self) -> &'static str {
        match self {
            Outcome::Sent { .. } => "SENT",
            Outcome::Failed { .. } => "FAILED",
            Outcome::Skipped { .. } => "SKIPPED",
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Outcome::Sent { .. } => None,
            Outcome::Failed { error, .. } | Outcome::Skipped { error } => Some(error),
        }
    }
}

fn skipped(error: impl Into<String>) -> Outcome {
    Outcome::Skipped {
        error: error.into(),
    }
}

fn failed(channel_id: Option<u32>, error: impl Into<String>) -> Outcome {
    Outcome::Failed {
        channel_id,
        error: error.into(),
    }
}

/// 解析实际使用的渠道配置：显式 ID 优先并校验类型与启用状态；0 取第一个
/// 启用的同类型渠道。一切"渠道用不了"都归 Skipped 语义。
async fn resolve_channel(
    db: &sea_orm::DatabaseConnection,
    type_name: &str,
    channel_id: u32,
) -> Result<chan::Model, String> {
    if channel_id == 0 {
        let row = chan::Entity::find()
            .filter(chan::Column::TypeColumn.eq(type_name))
            .filter(chan::Column::Status.eq("ON"))
            .one(db)
            .await
            .map_err(|e| format!("no enabled {type_name} channel configured: {e}"))?
            .ok_or_else(|| format!("no enabled {type_name} channel configured"))?;
        return Ok(row);
    }
    let row = chan::Entity::find_by_id(channel_id)
        .one(db)
        .await
        .map_err(|e| format!("channel [{channel_id}] not found: {e}"))?
        .ok_or_else(|| format!("channel [{channel_id}] not found"))?;
    if row.type_column.as_deref() != Some(type_name) {
        return Err(format!(
            "channel [{channel_id}] is not a {type_name} channel"
        ));
    }
    if row.status.as_deref() != Some("ON") {
        return Err(format!("channel [{channel_id}] is disabled"));
    }
    Ok(row)
}

/// 分发入口：按渠道类型投递一次。
pub async fn send(db: &sea_orm::DatabaseConnection, type_name: &str, req: &SendRequest) -> Outcome {
    match type_name {
        "EMAIL" => send_email(db, req).await,
        "WEBHOOK" => send_webhook(db, req).await,
        "SMS" => send_sms(db, req).await,
        other if crate::services::notification_channel::is_ext_type(other) => {
            // 声明边界：EXT 39 适配器（wecom/dingtalk/feishu 应用消息、
            // telegram、matrix……各有独立协议与 token 流）随独立里程碑接入；
            // schema 必填预检已过，这里明确报缺而不是静默吞掉。
            failed(
                Some(req.channel_id),
                "ext channel outbound adapters ride their own milestone; settings precheck passed, delivery not wired yet",
            )
        }
        _ => skipped(format!("unknown notification channel type {type_name:?}")),
    }
}

// —— EMAIL（SMTP） ——

async fn send_email(db: &sea_orm::DatabaseConnection, req: &SendRequest) -> Outcome {
    if req.target.trim().is_empty() {
        return failed(Some(req.channel_id), "email target is empty");
    }
    let row = match resolve_channel(db, "EMAIL", req.channel_id).await {
        Ok(row) => row,
        Err(e) => return skipped(e),
    };
    let host = row.smtp_host.clone().unwrap_or_default();
    if host.trim().is_empty() {
        return skipped(format!("channel [{}] has no smtp host configured", row.id));
    }
    let port = row.smtp_port.unwrap_or(465) as u16;
    let username = row.smtp_username.clone().unwrap_or_default();
    let password = crate::crypto::decrypt_if_needed(&row.smtp_password.clone().unwrap_or_default())
        .unwrap_or_default();
    let from = row.smtp_from.clone().unwrap_or_default();
    if from.trim().is_empty() {
        return skipped(format!("channel [{}] has no smtp from configured", row.id));
    }
    let from_name = row.smtp_from_name.clone().unwrap_or_default();
    let tls = row.smtp_tls.clone().unwrap_or_default();

    use lettre::message::Mailbox;
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::AsyncSmtpTransport;
    use lettre::AsyncTransport;
    use lettre::Message;

    let from_mailbox: Mailbox = match from.parse() {
        Ok(m) => m,
        Err(_) => {
            // 裸地址形态：补 display name 再解析一次
            match format!("{from_name} <{from}>").parse() {
                Ok(m) => m,
                Err(e) => return skipped(format!("channel [{}] smtp from invalid: {e}", row.id)),
            }
        }
    };
    let to_mailbox: Mailbox = match req.target.trim().parse() {
        Ok(m) => m,
        Err(e) => return failed(Some(row.id), format!("email target invalid: {e}")),
    };
    let email = match Message::builder()
        .from(from_mailbox)
        .to(to_mailbox)
        .subject(&req.title)
        .body(req.content.clone())
    {
        Ok(m) => m,
        Err(e) => return failed(Some(row.id), format!("build mail failed: {e}")),
    };

    use lettre::transport::smtp::client::Tls;
    // NONE 之外的形态都要 TlsParameters（证书域按 host）
    let tls_params = match lettre::transport::smtp::client::TlsParameters::new(host.clone()) {
        Ok(p) => p,
        Err(e) => return skipped(format!("channel [{}] tls parameters: {e}", row.id)),
    };
    let tls_mode = match tls.to_ascii_uppercase().as_str() {
        "SSL" => Tls::Wrapper(tls_params),
        "START_TLS" => Tls::Required(tls_params),
        "NONE" => Tls::None,
        other => {
            return skipped(format!(
                "channel [{}] smtp_tls {other:?} is not one of NONE/START_TLS/SSL",
                row.id
            ))
        }
    };

    let mut builder = match tls_mode {
        lettre::transport::smtp::client::Tls::None => {
            AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(&host)
        }
        _ => match AsyncSmtpTransport::<lettre::Tokio1Executor>::relay(&host) {
            Ok(b) => b,
            Err(e) => return failed(Some(row.id), format!("smtp relay build: {e}")),
        },
    };
    builder = builder.tls(tls_mode).port(port);
    if !username.is_empty() {
        builder = builder.credentials(Credentials::new(username, password));
    }
    let mailer = builder.build();

    match mailer.send(email).await {
        Ok(_) => Outcome::Sent { channel_id: row.id },
        Err(e) => failed(
            Some(row.id),
            format!("send mail via channel [{}] failed: {e}", row.id),
        ),
    }
}

// —— WEBHOOK ——

/// CUSTOM/NONE 风格且未配模板时的默认出站 JSON 形状：字段名与顺序都是存量
/// 对端已在验的东西——业务侧与对端的契约，形状留在本仓（上游 webhookPayload 同构）。
fn default_payload_json(req: &SendRequest) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "event_type".into(),
        serde_json::Value::String(req.event_type.clone()),
    );
    map.insert("title".into(), serde_json::Value::String(req.title.clone()));
    map.insert(
        "content".into(),
        serde_json::Value::String(req.content.clone()),
    );
    if req.params.contains_key("recipient_user_id") || req.params.contains_key("related_id") {
        // 由 Params 显式携带时才出现（omitempty 语义）
    }
    if let Some(uid) = req.params.get("recipient_user_id") {
        if let Ok(v) = uid.parse::<u32>() {
            map.insert("recipient_user_id".into(), serde_json::json!(v));
        }
    }
    if let Some(rid) = req.params.get("related_id") {
        if let Ok(v) = rid.parse::<u32>() {
            map.insert("related_id".into(), serde_json::json!(v));
        }
    }
    map.insert(
        "delivered_at".into(),
        serde_json::Value::String(rfc3339_now()),
    );
    serde_json::Value::Object(map)
}

fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// 出站风格（resolveStyle 同判）：空串按 CUSTOM（NULL 的语义就是"加列之前
/// 只有 CUSTOM 一种行为"）；认不出的值报错——拼错风格静默按自有方案发出去，
/// 对端只表现为"签名不对"，排查方向整个是反的。
fn resolve_style(raw: &str) -> Result<&'static str, String> {
    match raw {
        "" => Ok("CUSTOM"),
        "CUSTOM" => Ok("CUSTOM"),
        "NONE" => Ok("NONE"),
        "DINGTALK" => Ok("DINGTALK"),
        "FEISHU" => Ok("FEISHU"),
        "WECOM" => Ok("WECOM"),
        other => Err(format!(
            "unknown webhook sign style {other:?} (expected CUSTOM / NONE / DINGTALK / FEISHU / WECOM)"
        )),
    }
}

/// 该风格用哪个 body 字段判"对端真的收下了"（三家都会在 HTTP 200 里报失败）。
fn verdict_field(style: &str) -> Option<&'static str> {
    match style {
        "DINGTALK" | "WECOM" => Some("errcode"),
        "FEISHU" => Some("code"),
        _ => None,
    }
}

/// 钉钉：base64(hmac_sha256(key=secret, data="<ms>\n<secret>"))。
fn sign_dingtalk(secret: &str, timestamp: &str) -> String {
    use hmac::Mac;
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes())
        .expect("hmac accepts any key length");
    mac.update(format!("{timestamp}\n{secret}").as_bytes());
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// 飞书：base64(hmac_sha256(key="<s>\n<secret>", data=空))——与钉钉的
/// key/data 分工恰好相反，两条向量测试各钉一条防记反。
fn sign_feishu(secret: &str, timestamp: &str) -> String {
    use hmac::Mac;
    // key 即待签串、无更新数据（飞书协议形）——mac 不经 update 直取 finalize
    let mac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(format!("{timestamp}\n{secret}").as_bytes())
            .expect("hmac accepts any key length");
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// CUSTOM：hex(hmac_sha256(secret, "<ts>.<body>"))，时间戳折进签名。
fn sign_custom_body(secret: &str, timestamp: &str, body: &[u8]) -> String {
    use hmac::Mac;
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes())
        .expect("hmac accepts any key length");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// 占位符渲染：值一律按 JSON 字符串内容转义、不加引号——放在引号里的
/// {{title}} 与不放在引号里的 {{timestamp}} 都能用，引号归模板作者。
fn render_template(template: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

fn json_escape(s: &str) -> String {
    serde_json::to_string(s)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

/// 一次出站请求的渲染结果：发什么、签名放哪、用什么判成败。
struct Outbound {
    body: Vec<u8>,
    /// CUSTOM 风格的签名头；空则不带。
    timestamp_header: Option<(String, String)>,
    /// 钉钉拼 URL 的签名参数；None 表示不改地址。
    query: Option<Vec<(String, String)>>,
}

fn build_outbound(
    style: &str,
    secret: &str,
    payload_template: &str,
    req: &SendRequest,
) -> Result<Outbound, String> {
    let timestamp = if style == "DINGTALK" {
        chrono::Utc::now().timestamp_millis().to_string()
    } else {
        chrono::Utc::now().timestamp().to_string()
    };

    // bodySign 只在"签名不依赖正文"的风格里有值；有值才作为 {{sign}} 暴露。
    let mut body_sign = String::new();
    let mut outbound = Outbound {
        body: Vec::new(),
        timestamp_header: None,
        query: None,
    };
    match style {
        "DINGTALK" => {
            if !secret.is_empty() {
                body_sign = sign_dingtalk(secret, &timestamp);
                outbound.query = Some(vec![
                    ("timestamp".into(), timestamp.clone()),
                    ("sign".into(), body_sign.clone()),
                ]);
            }
        }
        "FEISHU" => {
            if !secret.is_empty() {
                body_sign = sign_feishu(secret, &timestamp);
            }
        }
        "CUSTOM" => {
            // 头里的时间戳先定下来：它不进正文，也就不可能形成循环。
            outbound.timestamp_header = Some((timestamp.clone(), String::new()));
        }
        _ => {} // NONE / WECOM：无签名（企微的凭据在 URL 的 key 上）
    }

    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    vars.insert("title".into(), json_escape(&req.title));
    vars.insert("content".into(), json_escape(&req.content));
    vars.insert("delivered_at".into(), json_escape(&rfc3339_now()));
    vars.insert("timestamp".into(), json_escape(&timestamp));
    vars.insert("nonce".into(), json_escape(&crate::token::new_jwt_id()));
    for (k, v) in &req.params {
        vars.insert(k.clone(), json_escape(v));
    }
    if !body_sign.is_empty() {
        vars.insert("sign".into(), json_escape(&body_sign));
    }

    let body: Vec<u8> = if style == "CUSTOM" || style == "NONE" {
        if payload_template.trim().is_empty() {
            serde_json::to_vec(&default_payload_json(req))
                .map_err(|e| format!("encode payload failed: {e}"))?
        } else {
            render_and_validate(payload_template.to_string(), &vars)?
        }
    } else {
        let template = if payload_template.trim().is_empty() {
            builtin_template(style).to_string()
        } else {
            payload_template.to_string()
        };
        render_and_validate(template, &vars)?
    };
    outbound.body = body;

    // 飞书把签名当正文字段：模板渲染之后并进顶层对象。
    if style == "FEISHU" && !secret.is_empty() {
        let mut map: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(
            &outbound.body,
        )
        .map_err(|_| {
            "feishu signature goes into the body, so the payload template must render a JSON object"
                .to_string()
        })?;
        map.insert(
            "timestamp".into(),
            serde_json::Value::String(timestamp.clone()),
        );
        map.insert("sign".into(), serde_json::Value::String(body_sign.clone()));
        outbound.body = serde_json::to_vec(&serde_json::Value::Object(map))
            .map_err(|e| format!("encode feishu envelope failed: {e}"))?;
    }

    if style == "CUSTOM" && !secret.is_empty() {
        let (ts, _) = outbound.timestamp_header.take().unwrap();
        let signature = format!("sha256={}", sign_custom_body(secret, &ts, &outbound.body));
        outbound.timestamp_header = Some((ts, signature));
    }

    Ok(outbound)
}

fn builtin_template(style: &str) -> &'static str {
    match style {
        "DINGTALK" => r#"{"msgtype":"text","text":{"content":"{{title}}\n{{content}}"}}"#,
        "FEISHU" => r#"{"msg_type":"text","content":{"text":"{{title}}\n{{content}}"}}"#,
        "WECOM" => r#"{"msgtype":"text","text":{"content":"{{title}}\n{{content}}"}}"#,
        _ => "",
    }
}

fn render_and_validate(
    template: String,
    vars: &BTreeMap<String, String>,
) -> Result<Vec<u8>, String> {
    let rendered = render_template(&template, vars);
    let valid: serde_json::Value = serde_json::from_str(&rendered).map_err(|_| {
        format!(
            "payload template rendered invalid JSON: {}",
            truncate(&rendered, 512)
        )
    })?;
    Ok(serde_json::to_vec(&valid).unwrap_or_default())
}

fn truncate(s: &str, limit: usize) -> String {
    if s.len() <= limit {
        return s.to_string();
    }
    let mut cut = limit;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &s[..cut])
}

async fn send_webhook(db: &sea_orm::DatabaseConnection, req: &SendRequest) -> Outcome {
    if req.target.trim().is_empty() {
        return failed(Some(req.channel_id), "webhook target url is empty");
    }
    let row = match resolve_channel(db, "WEBHOOK", req.channel_id).await {
        Ok(row) => row,
        Err(e) => return skipped(e),
    };
    if row
        .webhook_url
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return skipped(format!(
            "channel [{}] has no webhook_url configured",
            row.id
        ));
    }
    // 地址取自 SendRequest.Target（不是渠道行的 webhook_url）：台账的 target
    // 列回答"这条到底发去了哪里"；webhook_url 的作用域是登记值 + 预检门槛。
    let style = match resolve_style(&row.webhook_sign_style.clone().unwrap_or_default()) {
        Ok(s) => s.to_string(),
        Err(e) => return skipped(format!("channel [{}] {e}", row.id)),
    };
    let secret = crate::crypto::decrypt_if_needed(&row.webhook_secret.clone().unwrap_or_default())
        .unwrap_or_default();

    let outbound = match build_outbound(
        &style,
        &secret,
        &row.webhook_payload_template.clone().unwrap_or_default(),
        req,
    ) {
        Ok(o) => o,
        Err(e) => return skipped(format!("channel [{}] {e}", row.id)),
    };

    // SSRF 防线（拨号前判定）：私网/回环/元数据/内网后缀拒绝——SKIPPED 语义
    // （一次都没联系对端）。NOTIFICATION_WEBHOOK_ALLOW_PRIVATE=1 显式放行本机联调。
    if std::env::var("NOTIFICATION_WEBHOOK_ALLOW_PRIVATE").as_deref() != Ok("1") {
        if let Err(e) = super::net_guard::validate_outbound_url(&req.target) {
            return skipped(format!(
                "channel [{}] {e} (NOTIFICATION_WEBHOOK_ALLOW_PRIVATE=1 disables this check for local debugging)",
                row.id
            ));
        }
    }

    // 钉钉把签名拼在 URL query 上：保留对端原本要看的 access_token 等参数，
    // 只追加两个键——先定最终地址再构造请求。
    let mut endpoint = match reqwest::Url::parse(req.target.trim()) {
        Ok(u) => u,
        Err(e) => return failed(Some(row.id), format!("webhook url parse: {e}")),
    };
    if let Some(query) = &outbound.query {
        for (k, v) in query {
            endpoint.query_pairs_mut().append_pair(k, v);
        }
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())
        .unwrap_or_default();
    let mut request = client
        .post(endpoint)
        .header("Content-Type", "application/json; charset=utf-8")
        .header("User-Agent", "rushwind-webhook/1.0")
        .body(outbound.body.clone());
    if let Some((ts, sig)) = &outbound.timestamp_header {
        if !sig.is_empty() {
            request = request
                .header("X-Gw-Timestamp", ts)
                .header("X-Gw-Signature", sig);
        }
    }
    finish_webhook(row.id, &style, request.send().await).await
}

async fn finish_webhook(
    channel_id: u32,
    style: &str,
    response: Result<reqwest::Response, reqwest::Error>,
) -> Outcome {
    let mut resp = match response {
        Ok(r) => r,
        Err(e) => {
            return failed(
                Some(channel_id),
                format!("send webhook via channel [{channel_id}] failed: {e}"),
            )
        }
    };
    // 读满一个足够装下"对端那句结论"的量（4KB 上限，与上游 webhookAnswerBytes 同参）。
    let mut snippet = Vec::new();
    while let Ok(Some(chunk)) = resp.chunk().await {
        snippet.extend_from_slice(&chunk);
        if snippet.len() > 4 * 1024 {
            break;
        }
    }
    let status = resp.status();
    if !(status.is_success()) {
        return failed(
            Some(channel_id),
            format!(
                "peer answered {}: {}",
                status,
                truncate(&String::from_utf8_lossy(&snippet), 512)
            ),
        );
    }
    // 2xx 之后才轮到 body 说话：钉钉/飞书/企微在拦下消息时给的仍然是 200。
    if let Some(verdict) = verdict_field(style) {
        let answer: serde_json::Value = match serde_json::from_slice(&snippet) {
            Ok(v) => v,
            Err(_) => return Outcome::Sent { channel_id }, // 非 JSON：对端已用 2xx 表态，猜它的意思不如承认看不出
        };
        let code = answer.get(verdict).and_then(|v| v.as_i64()).unwrap_or(0);
        if code != 0 {
            let msg = answer
                .get("errmsg")
                .or_else(|| answer.get("msg"))
                .and_then(|v| v.as_str())
                .unwrap_or("no message");
            return failed(
                Some(channel_id),
                format!("provider answered {verdict}={code}: {msg}"),
            );
        }
    }
    Outcome::Sent { channel_id }
}

// —— SMS（阿里云 / 腾讯云） ——

/// 手机号归一（NormalizePhoneNumber 同判）：剥离空格/连字符/括号，+86 前缀
/// 摘除；主体 5-20 位纯数字。返回 (裸号, E.164)。
fn normalize_phone(raw: &str) -> Result<(String, String), String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '(' | ')'))
        .collect();
    if cleaned.is_empty() {
        return Err(format!("phone number {raw:?} is empty"));
    }
    let body = cleaned.strip_prefix('+').unwrap_or(&cleaned);
    let digits = body.bytes().filter(|b| b.is_ascii_digit()).count();
    if digits != body.len() || !(5..=20).contains(&digits) {
        return Err(format!(
            "phone number {raw:?} is not a valid MSISDN (5-20 digits, optional +86 prefix)"
        ));
    }
    if let Some(national) = cleaned.strip_prefix("+86") {
        return Ok((national.to_string(), cleaned.clone()));
    }
    Ok((cleaned.clone(), format!("+{cleaned}")))
}

/// 阿里云规范百分号编码（RFC3986）：空格用 %20（不是 +）、星号转义、
/// 波浪号不转义——QueryEscape 三处都与规范不同，逐条修回来。
fn aliyun_percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 阿里云 RPC 风格签名（HMAC-SHA1）：stringToSign = "POST&%2F&" +
/// percentEncode(按参数名排序的 canonical query)，密钥是 AccessKeySecret+"&"。
fn sign_aliyun_rpc(params: &BTreeMap<String, String>, access_secret: &str) -> String {
    use hmac::Mac;
    let canonical: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", aliyun_percent_encode(k), aliyun_percent_encode(v)))
        .collect();
    let string_to_sign = format!(
        "POST&{}&{}",
        aliyun_percent_encode("/"),
        aliyun_percent_encode(&canonical.join("&"))
    );
    let mut mac = hmac::Hmac::<sha1::Sha1>::new_from_slice(format!("{access_secret}&").as_bytes())
        .expect("hmac accepts any key length");
    mac.update(string_to_sign.as_bytes());
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// TC3-HMAC-SHA256 请求签名：四段派生（date → service → tc3_request →
/// stringToSign），Credential 里必须带 AccessKeyId。
fn sign_tencent_tc3(
    access_key_id: &str,
    access_secret: &str,
    timestamp: i64,
    payload: &[u8],
) -> String {
    use hmac::Mac;
    use sha2::Digest;
    let date = chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let service = "sms";
    let host = "sms.tencentcloudapi.com";
    let hashed_payload = hex::encode(sha2::Sha256::digest(payload));
    let canonical_request = format!(
        "POST\n/\n\ncontent-type:application/json; charset=utf-8\nhost:{host}\n\ncontent-type;host\n{hashed_payload}"
    );
    let string_to_sign = format!(
        "TC3-HMAC-SHA256\n{timestamp}\n{date}/{service}/tc3_request\n{}",
        hex::encode(sha2::Sha256::digest(canonical_request.as_bytes()))
    );
    let hmac = |key: &[u8], data: &[u8]| -> Vec<u8> {
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key).expect("hmac any key");
        mac.update(data);
        mac.finalize().into_bytes().to_vec()
    };
    let k_date = hmac(format!("TC3{access_secret}").as_bytes(), date.as_bytes());
    let k_service = hmac(&k_date, service.as_bytes());
    let k_signing = hmac(&k_service, b"tc3_request");
    let signature = hex::encode(hmac(&k_signing, string_to_sign.as_bytes()));
    format!(
        "TC3-HMAC-SHA256 Credential={access_key_id}/{date}/{service}/tc3_request, SignedHeaders=content-type;host, Signature={signature}"
    )
}

async fn send_sms(db: &sea_orm::DatabaseConnection, req: &SendRequest) -> Outcome {
    if req.target.trim().is_empty() {
        return failed(Some(req.channel_id), "sms target is empty");
    }
    // 没带 code 的投递在选账号之前就拒绝：这是路由/调用方的问题，不是渠道
    // 配置问题，记 FAILED——不该混进 SKIPPED 让管理员白查一遍配置。
    let Some(code) = req.params.get("code").filter(|c| !c.is_empty()) else {
        return failed(
            Some(req.channel_id),
            r#"sms channel requires params["code"]: SMS only carries verification-code events, the body is rendered by the provider-side approved template"#,
        );
    };
    let row = match resolve_channel(db, "SMS", req.channel_id).await {
        Ok(row) => row,
        Err(e) => return skipped(e),
    };
    let (national, _e164) = match normalize_phone(&req.target) {
        Ok(v) => v,
        Err(e) => return skipped(format!("channel [{}] {e}", row.id)),
    };
    let provider = row
        .sms_provider
        .clone()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let access_key_id = row.sms_access_key_id.clone().unwrap_or_default();
    let access_secret =
        crate::crypto::decrypt_if_needed(&row.sms_access_secret.clone().unwrap_or_default())
            .unwrap_or_default();
    let sign_name = row.sms_sign_name.clone().unwrap_or_default();
    let template_code = row.sms_template_code.clone().unwrap_or_default();

    let region = row.sms_region.clone().unwrap_or_default();
    let config = SmsConfig {
        channel_id: row.id,
        phone: &national,
        access_key_id: &access_key_id,
        access_secret: &access_secret,
        sign_name: &sign_name,
        template_code: &template_code,
        region: &region,
        app_id: &row.sms_app_id.clone().unwrap_or_default(),
        code,
    };
    match provider.as_str() {
        "ALIYUN" => send_sms_aliyun(&config).await,
        "TENCENT" => {
            if config.app_id.is_empty() {
                return skipped(format!(
                    "channel [{}] has no sms_app_id configured (TENCENT requires SmsSdkAppId)",
                    row.id
                ));
            }
            send_sms_tencent(&config).await
        }
        other => skipped(format!(
            "channel [{}] unknown sms provider {other:?} (expected ALIYUN / TENCENT)",
            row.id
        )),
    }
}

/// SMS 出站配置（上游 sms.Config 同构，channel_id 兼作台账回执）。
struct SmsConfig<'a> {
    channel_id: u32,
    phone: &'a str,
    access_key_id: &'a str,
    access_secret: &'a str,
    sign_name: &'a str,
    template_code: &'a str,
    region: &'a str,
    /// TENCENT 专用（SmsSdkAppId）。
    app_id: &'a str,
    code: &'a str,
}

async fn send_sms_aliyun(cfg: &SmsConfig<'_>) -> Outcome {
    let channel_id = cfg.channel_id;
    let phone = cfg.phone;
    let access_key_id = cfg.access_key_id;
    let access_secret = cfg.access_secret;
    let sign_name = cfg.sign_name;
    let template_code = cfg.template_code;
    let region = cfg.region;
    let code = cfg.code;
    let mut params: BTreeMap<String, String> = BTreeMap::new();
    params.insert("AccessKeyId".into(), access_key_id.to_string());
    params.insert("Action".into(), "SendSms".into());
    params.insert("Format".into(), "JSON".into());
    params.insert("PhoneNumbers".into(), phone.to_string());
    params.insert(
        "RegionId".into(),
        if region.is_empty() {
            "cn-hangzhou".into()
        } else {
            region.to_string()
        },
    );
    params.insert("SignName".into(), sign_name.to_string());
    params.insert("SignatureMethod".into(), "HMAC-SHA1".into());
    params.insert("SignatureNonce".into(), crate::token::new_jwt_id());
    params.insert("SignatureVersion".into(), "1.0".into());
    params.insert("TemplateCode".into(), template_code.to_string());
    params.insert(
        "TemplateParam".into(),
        serde_json::json!({ "code": code }).to_string(),
    );
    params.insert(
        "Timestamp".into(),
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    );
    params.insert("Version".into(), "2017-05-26".into());
    params.insert("Signature".into(), sign_aliyun_rpc(&params, access_secret));

    let form: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", aliyun_percent_encode(k), aliyun_percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let response = client
        .post("https://dysmsapi.aliyuncs.com/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await;
    let mut resp = match response {
        Ok(r) => r,
        Err(e) => {
            return failed(
                Some(channel_id),
                format!("send sms via channel [{channel_id}] failed: {e}"),
            )
        }
    };
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = resp.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() > 4 * 1024 {
            break;
        }
    }
    if !resp.status().is_success() {
        return failed(
            Some(channel_id),
            format!(
                "peer answered {}: {}",
                resp.status(),
                truncate(&String::from_utf8_lossy(&body), 512)
            ),
        );
    }
    let answer: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return failed(
                Some(channel_id),
                format!(
                    "peer answer is not JSON: {}",
                    truncate(&String::from_utf8_lossy(&body), 512)
                ),
            )
        }
    };
    let answer_code = answer.get("Code").and_then(|v| v.as_str()).unwrap_or("");
    if answer_code != "OK" {
        let message = answer
            .get("Message")
            .and_then(|v| v.as_str())
            .unwrap_or("no message");
        return failed(
            Some(channel_id),
            format!("provider answered Code={answer_code}: {message}"),
        );
    }
    Outcome::Sent { channel_id }
}

async fn send_sms_tencent(cfg: &SmsConfig<'_>) -> Outcome {
    let channel_id = cfg.channel_id;
    let phone = cfg.phone;
    let access_key_id = cfg.access_key_id;
    let access_secret = cfg.access_secret;
    let sign_name = cfg.sign_name;
    let template_code = cfg.template_code;
    let region = cfg.region;
    let app_id = cfg.app_id;
    let code = cfg.code;
    let region = if region.is_empty() { "" } else { region };
    let payload = serde_json::json!({
        "PhoneNumberSet": [phone],
        "SmsSdkAppId": app_id,
        "SignName": sign_name,
        "TemplateId": template_code,
        "TemplateParamSet": [code],
    });
    let payload = payload.to_string().into_bytes();
    let now = chrono::Utc::now().timestamp();
    let mut request = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default()
        .post("https://sms.tencentcloudapi.com/")
        .header("Content-Type", "application/json; charset=utf-8")
        .header("X-TC-Action", "SendSms")
        .header("X-TC-Version", "2021-01-11")
        .header("X-TC-Timestamp", now.to_string())
        .header(
            "Authorization",
            sign_tencent_tc3(access_key_id, access_secret, now, &payload),
        )
        .body(payload);
    if !region.is_empty() {
        request = request.header("X-TC-Region", region);
    }
    let response = request.send().await;
    let mut resp = match response {
        Ok(r) => r,
        Err(e) => {
            return failed(
                Some(channel_id),
                format!("send sms via channel [{channel_id}] failed: {e}"),
            )
        }
    };
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = resp.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() > 4 * 1024 {
            break;
        }
    }
    if !resp.status().is_success() {
        return failed(
            Some(channel_id),
            format!(
                "peer answered {}: {}",
                resp.status(),
                truncate(&String::from_utf8_lossy(&body), 512)
            ),
        );
    }
    let answer: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return failed(
                Some(channel_id),
                format!(
                    "peer answer is not JSON: {}",
                    truncate(&String::from_utf8_lossy(&body), 512)
                ),
            )
        }
    };
    if let Some(err) = answer.pointer("/Response/Error") {
        let code = err.get("Code").and_then(|v| v.as_str()).unwrap_or("");
        let message = err.get("Message").and_then(|v| v.as_str()).unwrap_or("");
        return failed(
            Some(channel_id),
            format!("provider answered Error={code}: {message}"),
        );
    }
    let status = answer.pointer("/Response/SendStatusSet/0");
    let Some(status) = status else {
        return failed(
            Some(channel_id),
            format!(
                "peer answered without SendStatusSet: {}",
                truncate(&String::from_utf8_lossy(&body), 512)
            ),
        );
    };
    // 腾讯云单号 Code=="Ok"（这个 Ok 大小写与阿里云不同）才是发出去了。
    let status_code = status.get("Code").and_then(|v| v.as_str()).unwrap_or("");
    if status_code != "Ok" {
        let message = status
            .get("Message")
            .and_then(|v| v.as_str())
            .unwrap_or("no message");
        return failed(
            Some(channel_id),
            format!("provider answered Code={status_code}: {message}"),
        );
    }
    Outcome::Sent { channel_id }
}

/// 一次"落台账-投递-回写"的完整同步投递（上游 deliver 同构）：台账先落
/// SENDING 行，投递一次（attempts=1），按结论回写。回写失败**不回错**给业务
/// 侧——信已出去，台账停在 SENDING 由清扫定案；写失败留痕可观测。
pub async fn deliver_once(
    db: &sea_orm::DatabaseConnection,
    channel_type: &str,
    req: &SendRequest,
    operator_id: u32,
) -> Result<(u32, Outcome), StatusError> {
    use sea_orm::ActiveModelTrait;
    let delivery = crate::data::repos::NotificationDeliveryRepo::new(db)
        .insert(crate::data::sys_notification_deliveries::ActiveModel {
            event_type: Set(Some(req.event_type.clone())),
            channel: Set(Some(channel_type.to_string())),
            channel_id: Set((req.channel_id > 0).then_some(req.channel_id)),
            recipient_user_id: Set(None),
            related_id: Set(None),
            target: Set(Some(req.target.clone())),
            status: Set(Some("SENDING".into())),
            attempts: Set(Some(1)),
            created_by: Set(Some(operator_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        })
        .await?;
    let delivery_id = delivery.id;
    let outcome = send(db, channel_type, req).await;
    let mut a: crate::data::sys_notification_deliveries::ActiveModel = delivery.into();
    a.status = Set(Some(outcome.status().into()));
    a.last_error = Set(outcome.error().map(str::to_string));
    let picked = match &outcome {
        Outcome::Sent { channel_id } => Some(*channel_id),
        Outcome::Failed { channel_id, .. } => *channel_id,
        Outcome::Skipped { .. } => None,
    };
    if let Some(cid) = picked {
        a.channel_id = Set(Some(cid));
    }
    a.updated_at = Set(Some(crate::data::now()));
    if let Err(e) = a.update(db).await {
        eprintln!("[notify] mark result for delivery failed: {e:?}");
    }
    Ok((delivery_id, outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_signature_vectors_pinned() {
        // 取自上游 style_test.go 的两条文档向量：钉钉是 key=secret /
        // data="<ts>\n<secret>"，飞书是 key="<ts>\n<secret>" / data=空——
        // 分工记反时对端只回一句 "sign not match"，向量把方向钉死。
        assert_eq!(
            sign_dingtalk("xxx", "1661860880"),
            "dIUMcehSFkPlWOGqwoP5EFcartyN67QSY9wbVW+hTbE="
        );
        assert_eq!(
            sign_feishu("xxx", "1661860880"),
            "QnWVTSBe6FmQDE0bG6X0mURbI+DnvVyu1h+j5dHOjrU="
        );
    }

    #[test]
    fn custom_sign_is_timestamp_folded_body_hmac() {
        let sig = sign_custom_body("s3cret", "1700000000", br#"{"a":1}"#);
        assert_eq!(sig.len(), 64); // hex(sha256)
                                   // 同参重算一致、换 body 即变
        assert_eq!(sig, sign_custom_body("s3cret", "1700000000", br#"{"a":1}"#));
        assert_ne!(sig, sign_custom_body("s3cret", "1700000000", br#"{"a":2}"#));
    }

    #[test]
    fn style_resolution_is_case_strict() {
        assert_eq!(resolve_style("").unwrap(), "CUSTOM");
        for raw in ["CUSTOM", "NONE", "DINGTALK", "FEISHU", "WECOM"] {
            assert_eq!(resolve_style(raw).unwrap(), raw);
        }
        // 认不出的值报错（大小写/拼写变体都拒）：静默兜成 CUSTOM 会让排查方向整个反掉
        for raw in [
            "custom", "DINGTAKL", "DingTalk", "dingtalk", "  FEISHU", "SMS",
        ] {
            assert!(resolve_style(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn outbound_bodies_per_style() {
        let mut params = BTreeMap::new();
        params.insert("recipient_user_id".to_string(), "42".to_string());
        let req = SendRequest {
            target: "https://hooks.example.com/x".into(),
            title: "标题<t>".into(),
            content: "正文&内容".into(),
            channel_id: 9,
            event_type: "CHANNEL_TEST_WEBHOOK".into(),
            params,
        };
        // 钉钉：text 形状，签名进 query
        let o = build_outbound("DINGTALK", "sec", "", &req).unwrap();
        let body = String::from_utf8(o.body).unwrap();
        assert!(body.contains(r#""msgtype":"text""#), "{body}");
        assert!(body.contains("标题<t>"), "title 原文经 JSON 转义保留");
        assert_eq!(verdict_field("DINGTALK"), Some("errcode"));
        assert!(o.query.as_ref().unwrap().len() == 2);
        // CUSTOM：默认出站体带 event_type/recipient_user_id/delivered_at
        let o = build_outbound("CUSTOM", "", "", &req).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&o.body).unwrap();
        assert_eq!(v["event_type"], "CHANNEL_TEST_WEBHOOK");
        assert_eq!(v["recipient_user_id"], 42);
        assert!(v["delivered_at"].as_str().is_some());
        // CUSTOM + secret：签名进头、时间戳折进签名
        let o = build_outbound("CUSTOM", "sec", "", &req).unwrap();
        let (ts, sig) = o.timestamp_header.clone().unwrap();
        assert!(sig.starts_with("sha256="));
        assert_eq!(
            sig,
            format!("sha256={}", sign_custom_body("sec", &ts, &o.body))
        );
        // 飞书：签名并入顶层对象
        let o = build_outbound("FEISHU", "sec", "", &req).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&o.body).unwrap();
        assert!(v["timestamp"].as_str().is_some());
        assert!(v["sign"].as_str().is_some());
        assert_eq!(verdict_field("FEISHU"), Some("code"));
        // 坏模板：渲染不出合法 JSON 即拒（SKIPPED 语义）
        assert!(build_outbound("NONE", "", "{oops", &req).is_err());
    }

    #[test]
    fn aliyun_encoding_matches_rfc3986() {
        assert_eq!(aliyun_percent_encode("a b"), "a%20b");
        assert_eq!(aliyun_percent_encode("a*b"), "a%2Ab");
        assert_eq!(aliyun_percent_encode("a~b"), "a~b");
        assert_eq!(aliyun_percent_encode("a+b"), "a%2Bb");
        assert_eq!(aliyun_percent_encode("中文"), "%E4%B8%AD%E6%96%87");
    }

    #[test]
    fn aliyun_rpc_signature_is_deterministic() {
        let mut params = BTreeMap::new();
        params.insert("Action".to_string(), "SendSms".to_string());
        params.insert("AccessKeyId".to_string(), "testId".to_string());
        let sig = sign_aliyun_rpc(&params, "testSecret");
        assert_eq!(sig, sign_aliyun_rpc(&params, "testSecret"));
        assert_ne!(sig, sign_aliyun_rpc(&params, "otherSecret"));
        assert_eq!(sig.len(), 28); // base64(hmac-sha1) = 28 字符
    }

    #[test]
    fn tc3_signature_shape_and_determinism() {
        let payload = br#"{"PhoneNumberSet":["+8613800138000"]}"#;
        let sig = sign_tencent_tc3("AKIDtest", "secret", 1_700_000_000, payload);
        assert!(sig.starts_with("TC3-HMAC-SHA256 Credential=AKIDtest/2023-11-14/sms/tc3_request, SignedHeaders=content-type;host, Signature="), "{sig}");
        assert_eq!(
            sig,
            sign_tencent_tc3("AKIDtest", "secret", 1_700_000_000, payload)
        );
        assert_ne!(
            sig,
            sign_tencent_tc3("AKIDtest", "secret", 1_700_000_001, payload)
        );
    }

    #[test]
    fn phone_normalization_strips_and_validates() {
        assert_eq!(
            normalize_phone("+86 138-0013-8000").unwrap().0,
            "13800138000"
        );
        assert_eq!(normalize_phone("13800138000").unwrap().1, "+13800138000");
        assert!(normalize_phone("abc").is_err());
        assert!(normalize_phone("123").is_err()); // < 5 位
        assert!(normalize_phone("").is_err());
    }
}
