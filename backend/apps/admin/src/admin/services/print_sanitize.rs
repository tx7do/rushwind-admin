//! 打印渲染（headless Chrome）送渲染前的 HTML 清洗（P1-11，上游
//! print_template_sanitize.go 同语义）。
//!
//! RenderPdf 接受任意用户 HTML 直送 Chromium（about:blank + set_content）：
//! - `<iframe src="http://169.254.169.254/latest/meta-data/">` 的响应会被渲染
//!   进 PDF 返还给攻击者——一个"可读的 SSRF 原语"，云元数据/内网 HTTP 服务
//!   一锅端；
//! - `<script>` 在无 CSP 的 about:blank 里照常执行（n-day 前置面）。
//!
//! 处置：脚本/嵌入类元素与事件属性整体剥除，资源 URL 指向私网/回环/链路
//! 本地/元数据地址的**拒绝**（返回 Err，调用方 400——不静默改写）。URL 判定
//! 面与 Agent HTTP 节点同源（[`super::net_guard`]）。

use lol_html::{element, HtmlRewriter, Settings};

/// 渲染前整体剥除的元素：脚本执行与子资源加载面。iframe 是 SSRF 回读主
/// 通道；link/meta 的 stylesheet/CSP 覆盖在打印场景非必需；base 重写相对
/// URL 的落点，一并剥除。
fn is_strip_element(tag: &str) -> bool {
    matches!(
        tag,
        "script"
            | "iframe"
            | "frame"
            | "frameset"
            | "object"
            | "embed"
            | "applet"
            | "link"
            | "meta"
            | "base"
    )
}

/// 需要做 URL 校验的元素/属性对（其余元素的 src/href 视具体标签）。
fn is_url_attribute(tag: &str, attr: &str) -> bool {
    match tag {
        "a" | "area" => attr == "href",
        "img" | "audio" | "source" | "input" => attr == "src",
        "video" => attr == "src" || attr == "poster",
        "form" => attr == "action",
        "td" | "table" | "body" => attr == "background",
        _ => false,
    }
}

/// scheme 形态探测（与 Go url.Parse 同判）：`[a-zA-Z][a-zA-Z0-9+.-]*:` 前缀
/// 即绝对 URL；其余（"/a.png"、"pic.png"、"#frag"）是相对形态。
fn detect_scheme(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b':' {
            return Some(raw[..i].to_ascii_lowercase());
        }
        if !(b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.') {
            return None;
        }
        i += 1;
    }
    None
}

/// 校验资源 URL（checkPrintResourceURL 同语义）：
/// - 非 http(s) 的绝对 scheme（javascript:/data:/vbscript:/file: 等）一律拒绝；
/// - 相对 URL 在 about:blank 源下不可解析（本就打不开），放行；
/// - http(s)：主机为回环/私网/链路本地/元数据地址，或解析到私网段 → 拒绝。
fn check_resource_url(raw: &str) -> Result<(), String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(());
    }
    let Some(scheme) = detect_scheme(raw) else {
        // "//host/x" 形态（protocol-relative）：上游解析出空 scheme + host →
        // 落 scheme 检查即拒；其余相对 URL 放行
        if raw.starts_with("//") {
            return Err("blocked url scheme \"\"".into());
        }
        return Ok(());
    };
    if scheme != "http" && scheme != "https" {
        return Err(format!("blocked url scheme {scheme:?}"));
    }
    let u = reqwest::Url::parse(raw).map_err(|_| format!("invalid resource url {raw:?}"))?;
    let host = u.host_str().unwrap_or_default().to_string();
    if host.is_empty() {
        return Err(format!("invalid resource url {raw:?}"));
    }
    if super::net_guard::is_blocked_host(&host) {
        return Err(format!(
            "blocked resource host {host:?} (private/loopback/metadata)"
        ));
    }
    if host.parse::<std::net::IpAddr>().is_err() {
        use std::net::ToSocketAddrs as _;
        let addrs: Vec<std::net::SocketAddr> = (host.as_str(), 80)
            .to_socket_addrs()
            .map_err(|_| format!("cannot resolve resource host {host:?}"))?
            .collect();
        for a in &addrs {
            if super::net_guard::is_blocked_ip(&a.ip()) {
                return Err(format!(
                    "blocked resource host {host:?} (resolves to private address)"
                ));
            }
        }
    }
    Ok(())
}

/// 清洗用户 HTML 后返回可安全送 Chromium 的版本。返回 Err 表示发现被禁
/// 内容（调用方应 400 拒绝而不是静默改写）。
pub fn sanitize_print_html(input: &str) -> Result<String, String> {
    let blocked: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
    let mut output = Vec::with_capacity(input.len());
    {
        let mut rewriter = HtmlRewriter::new(
            Settings {
                element_content_handlers: vec![element!("*", |el| {
                    let tag = el.tag_name().to_ascii_lowercase();
                    if is_strip_element(&tag) {
                        el.remove();
                        return Ok(());
                    }
                    // 事件处理器与 srcset 一律剥除；URL 属性逐个校验
                    let mut to_remove: Vec<String> = Vec::new();
                    for attr in el.attributes() {
                        let name = attr.name().to_ascii_lowercase();
                        if name.starts_with("on") || name == "srcset" {
                            to_remove.push(name);
                            continue;
                        }
                        if is_url_attribute(&tag, &name) {
                            let value = attr.value();
                            if let Err(err) = check_resource_url(&value) {
                                blocked.replace(Some(err));
                                return Ok(());
                            }
                        }
                    }
                    for name in to_remove {
                        el.remove_attribute(&name);
                    }
                    Ok(())
                })],
                ..Settings::default()
            },
            |c: &[u8]| output.extend_from_slice(c),
        );
        rewriter
            .write(input.as_bytes())
            .map_err(|e| format!("printrender: sanitizer: {e}"))?;
        rewriter
            .end()
            .map_err(|e| format!("printrender: sanitizer end: {e}"))?;
    }
    if let Some(err) = blocked.into_inner() {
        return Err(err);
    }
    Ok(String::from_utf8_lossy(&output).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_detection_mirrors_go_url_parse() {
        // 字母开头 + [A-Za-z0-9+.-]* 才成 scheme；其余是相对形态
        assert_eq!(detect_scheme("https://a").as_deref(), Some("https"));
        assert_eq!(
            detect_scheme("javascript:alert(1)").as_deref(),
            Some("javascript")
        );
        assert_eq!(detect_scheme("DATA:text/html,x").as_deref(), Some("data"));
        assert_eq!(detect_scheme("a+b-c.d:x").as_deref(), Some("a+b-c.d"));
        assert!(detect_scheme("/path/x").is_none());
        assert!(detect_scheme("pic.png").is_none());
        assert!(detect_scheme("#frag").is_none());
        assert!(detect_scheme("//host/x").is_none());
        assert!(detect_scheme("1abc:x").is_none()); // 数字开头不是 scheme
        assert!(detect_scheme("").is_none());
    }

    #[test]
    fn strip_elements_and_event_handlers() {
        let out = sanitize_print_html(
            r#"<html><head><script>alert(1)</script><meta charset="utf-8"></head>
               <body onload="steal()"><p onclick="x()">hi</p>
               <iframe src="http://10.0.0.1/"></iframe></body></html>"#,
        )
        .unwrap();
        assert!(!out.contains("script"), "{out}");
        assert!(!out.contains("alert"), "{out}");
        assert!(!out.contains("meta"), "{out}");
        assert!(!out.contains("iframe"), "{out}");
        assert!(!out.contains("onload"), "{out}");
        assert!(!out.contains("onclick"), "{out}");
        assert!(out.contains("hi"), "{out}"); // 正文保留
    }

    #[test]
    fn dangerous_schemes_rejected_not_rewritten() {
        for html in [
            r#"<a href="javascript:alert(1)">x</a>"#,
            r#"<img src="data:text/html;base64,PHNjcmlwdD4=">"#,
            r#"<a href="vbscript:x">y</a>"#,
        ] {
            assert!(
                sanitize_print_html(html).is_err(),
                "{html} must be rejected"
            );
        }
    }

    #[test]
    fn private_hosts_rejected() {
        for html in [
            r#"<img src="http://127.0.0.1/secret.png">"#,
            r#"<img src="http://169.254.169.254/latest/meta-data/">"#,
            r#"<a href="http://10.0.0.5/intranet">link</a>"#,
            r#"<img src="http://printer.local/x.png">"#,
        ] {
            let err = sanitize_print_html(html).unwrap_err();
            assert!(
                err.contains("blocked resource host") || err.contains("blocked url scheme"),
                "{html}: {err}"
            );
        }
    }

    #[test]
    fn relative_urls_and_public_hosts_pass() {
        sanitize_print_html(
            r#"<img src="/static/logo.png"><img src="pic.png"><a href="/next">n</a>"#,
        )
        .unwrap();
        // 公网域名（解析失败也放行失败路径?——不，解析失败拒）
        // 本测试只锁相对形态与形参校验，不做真实网络解析
    }

    #[test]
    fn srcset_stripped_and_clean_doc_roundtrips() {
        let out = sanitize_print_html(
            r#"<img src="/a.png" srcset="/a.png 2x" alt="logo"><table background="/bg.png"><tr><td>c</td></tr></table>"#,
        )
        .unwrap();
        assert!(!out.contains("srcset"), "{out}");
        assert!(out.contains("/a.png"), "{out}");
        assert!(out.contains("background"), "{out}"); // 合法 URL 属性保留
    }
}
