//! 出站 URL 的 SSRF 防线（fail-closed）——与上游 RenderPdf 清洗链同源
//! （isBlockedPrintHost / isBlockedPrintIP），HTTP 节点（Agent 编排出站）
//! 与 RenderPdf（⑦ 安全批次接入）共用同一判定面：scheme 白名单 + 主机
//! 字面量/后缀判定 + 解析后逐 IP 复核私网/回环/链路本地/元数据段。
//! 与上游同样存在解析与请求两次解析之间的 TOCTOU 窗口（DNS 重绑定），
//! 强度一致、接受同一权衡。

/// 主机字面量/后缀判定：localhost 及其子域、.local（mDNS）、.internal
/// （内网约定后缀）一律拒；IP 字面量直接进 IP 层判定。
pub fn is_blocked_host(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    if h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
    {
        return true;
    }
    match h.parse::<std::net::IpAddr>() {
        Ok(ip) => is_blocked_ip(&ip),
        Err(_) => false,
    }
}

/// IP 层判定：回环/私网/链路本地（单播+组播）/未指定/组播 + CGNAT
/// （100.64.0.0/10——部分云元数据/内部服务落在该段）。
pub fn is_blocked_ip(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fe80::/10 链路本地（is_unicast_link_local 1.84 才稳定，
                // MSRV 1.81 手写判定）
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// http 节点出站 URL 校验：解析 → scheme 白名单（http/https）→ 主机字面量
/// /后缀 → 域名解析后逐 IP 复核。解析失败也是 fail-closed（不能因为"查不到"
/// 就放行）。
pub fn validate_outbound_url(raw: &str) -> Result<(), String> {
    let u = reqwest::Url::parse(raw).map_err(|_| format!("invalid outbound url {raw:?}"))?;
    if u.host_str().unwrap_or_default().is_empty() {
        return Err(format!("invalid outbound url {raw:?}"));
    }
    let scheme = u.scheme().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!("blocked outbound url scheme {scheme:?}"));
    }
    let host = u.host_str().unwrap_or_default().to_string();
    if is_blocked_host(&host) {
        return Err(format!(
            "blocked outbound host {host:?} (private/loopback/metadata)"
        ));
    }
    if host.parse::<std::net::IpAddr>().is_err() {
        // 域名：解析后逐 IP 复核（上游 net.LookupIP 同型；同步解析与上游
        // 阻塞式 goroutine 内 LookupIP 的强度一致）。
        use std::net::ToSocketAddrs as _;
        let addrs: Vec<std::net::SocketAddr> = (host.as_str(), 80)
            .to_socket_addrs()
            .map_err(|_| format!("cannot resolve outbound host {host:?}"))?
            .collect();
        if addrs.is_empty() {
            return Err(format!("cannot resolve outbound host {host:?}"));
        }
        for a in &addrs {
            if is_blocked_ip(&a.ip()) {
                return Err(format!(
                    "blocked outbound host {host:?} (resolves to private address)"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_literal_and_suffix_rules() {
        assert!(is_blocked_host("localhost"));
        assert!(is_blocked_host("api.localhost"));
        assert!(is_blocked_host("printer.local"));
        assert!(is_blocked_host("svc.internal"));
        assert!(is_blocked_host("127.0.0.1"));
        assert!(is_blocked_host("169.254.169.254"));
        assert!(is_blocked_host("10.0.0.5"));
        assert!(is_blocked_host("192.168.1.1"));
        assert!(is_blocked_host("172.16.0.9"));
        assert!(is_blocked_host("100.77.12.3")); // CGNAT
        assert!(is_blocked_host("::1"));
        assert!(is_blocked_host("LOCALHOST")); // 大小写归一
        assert!(is_blocked_host("localhost.")); // 尾点归一
        assert!(!is_blocked_host("example.com"));
        assert!(!is_blocked_host("8.8.8.8"));
        assert!(!is_blocked_host("notlocalhost")); // 无点邻接不算后缀
    }

    #[test]
    fn ip_boundary_edges() {
        // CGNAT 段界：100.63 不拦、100.64 起拦、100.127 末拦、100.128 出段
        assert!(!is_blocked_ip(&"100.63.255.255".parse().unwrap()));
        assert!(is_blocked_ip(&"100.64.0.0".parse().unwrap()));
        assert!(is_blocked_ip(&"100.127.255.255".parse().unwrap()));
        assert!(!is_blocked_ip(&"100.128.0.0".parse().unwrap()));
        // v6 链路本地 fe80::/10：fe80 与 febf 均拦，fec0 出段
        assert!(is_blocked_ip(&"fe80::1".parse().unwrap()));
        assert!(is_blocked_ip(&"febf::1".parse().unwrap()));
        assert!(!is_blocked_ip(&"fec0::1".parse().unwrap()));
        // v6 回环/未指定/组播
        assert!(is_blocked_ip(&"::1".parse().unwrap()));
        assert!(is_blocked_ip(&"::".parse().unwrap()));
        assert!(is_blocked_ip(&"ff02::1".parse().unwrap()));
        // 公网不拦
        assert!(!is_blocked_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!is_blocked_ip(&"2606:4700::1".parse().unwrap()));
    }

    #[test]
    fn outbound_url_rules_fail_closed() {
        // 回环 / 元数据 / 私网 / scheme / 内网后缀
        for raw in [
            "http://127.0.0.1/x",
            "http://localhost/x",
            "http://[::1]/x",
            "http://169.254.169.254/latest/meta-data/",
            "http://10.0.0.5/x",
            "http://192.168.1.1/x",
            "http://172.16.0.9/x",
            "ftp://example.com/file",
            "http://api.internal/x",
            "not-a-url",
        ] {
            assert!(validate_outbound_url(raw).is_err(), "{raw} must be blocked");
        }
        let err = validate_outbound_url("ftp://example.com/file").unwrap_err();
        assert!(err.contains("scheme"));
        let err = validate_outbound_url("http://api.internal/x").unwrap_err();
        assert!(err.contains("blocked outbound host"));
        // 无法解析的域名：fail-closed（解析失败也算拒）
        assert!(validate_outbound_url("http://this-host-does-not-exist.invalid/x").is_err());
    }
}
