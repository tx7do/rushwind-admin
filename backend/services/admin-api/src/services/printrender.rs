//! 服务端打印渲染：把前端渲染管线（go-wind-print 内核）产出的最终 HTML 用
//! 本机 Chrome/Edge（chromiumoxide 驱 CDP，自动探测安装路径）打印成 PDF。
//!
//! 分工边界：分页/毫米布局/字体全部由前端内核完成（两遍渲染：测量→分页→
//! 最终 HTML），本模块只做 HTML → PDF 这一步（Page.printToPDF），纸张宽高
//! 与方向由调用方按模板元数据传入——保证"预览即所得"（同一 Chromium 引擎），
//! 后端不重排、不理解模板。
//!
//! 渲染器留接口（[`Renderer`]）：默认本机 Chrome 的 CDP 实现；部署侧如偏好
//! 伴生容器（Playwright + Chromium 镜像），实现 [`Renderer`] 换入即可。

use std::sync::OnceLock;
use std::time::Duration;

/// 一次 PDF 打印的参数。
pub struct PrintOptions {
    /// 前端内核产出的最终文档（已完成分页），应为完整 `<html>` 文档。
    pub html: String,
    /// 纸张尺寸（毫米），前端按模板 paperSize 解析后传入。
    pub paper_width_mm: f64,
    pub paper_height_mm: f64,
    /// 横向（前端按模板方向交换宽高后传 false，避免二次旋转）。
    pub landscape: bool,
    /// 打印背景色/背景图（printToPDF 默认不打印背景）。
    pub background: bool,
}

/// HTML → PDF 渲染器接口（可换远程 print-render 服务实现）。
pub trait Renderer: Send + Sync {
    fn render_pdf(
        &self,
        opts: PrintOptions,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, String>> + Send;
}

/// 浏览器查找：CHROME_PATH 环境变量 → PATH 探测 → 常见安装路径 → Edge 兜底。
fn find_browser() -> Result<String, String> {
    if let Ok(p) = std::env::var("CHROME_PATH") {
        let p = p.trim().to_string();
        if !p.is_empty() && std::path::Path::new(&p).exists() {
            return Ok(p);
        }
    }
    for name in [
        "chrome",
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
    ] {
        if let Ok(path) = which_look_path(name) {
            return Ok(path);
        }
    }
    let candidates = [
        // Windows 服务器常无 Chrome，Edge 同 Chromium 协议兜底。
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        "/usr/bin/microsoft-edge",
    ];
    for p in candidates {
        if std::path::Path::new(p).exists() {
            return Ok(p.to_string());
        }
    }
    Err(
        "printrender: no Chrome/Edge found (set CHROME_PATH or install chromium in the image)"
            .into(),
    )
}

/// PATH 探测（`which` 的最小实现；无 PATH 命中返回 Err）。
fn which_look_path(name: &str) -> Result<String, String> {
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        let candidate = std::path::Path::new(dir).join(name);
        if candidate.is_file() {
            return Ok(candidate.to_string_lossy().into_owned());
        }
    }
    Err(format!("{name} not on PATH"))
}

/// Linux 下 root 运行时 Chrome 需 --no-sandbox 才能启动（容器默认用户）；
/// PRINTRENDER_NO_SANDBOX=0 可强制关闭该行为。
fn needs_no_sandbox() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    if std::env::var("PRINTRENDER_NO_SANDBOX").as_deref() == Ok("0") {
        return false;
    }
    // 无 geteuid 稳定跨平台口；libc 不入依赖树，读 /proc 自我状态兜底。
    std::fs::read_to_string("/proc/self/status")
        .map(|s| {
            s.lines()
                .any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(1) == Some("0"))
        })
        .unwrap_or(false)
}

/// 进程级复用的浏览器实例（与上游 ChromeRenderer 的 allocator 复用同语义：
/// 一次启动、多页渲染；打印服务无网络/扩展需求，收窄攻击面）。
static BROWSER: OnceLock<chromiumoxide::Browser> = OnceLock::new();

async fn browser() -> Result<&'static chromiumoxide::Browser, String> {
    if let Some(b) = BROWSER.get() {
        return Ok(b);
    }
    let path = find_browser()?;
    let mut config = chromiumoxide::BrowserConfig::builder()
        .chrome_executable(path)
        // 打印服务无网络/扩展需求：收窄攻击面
        .arg("--disable-extensions")
        .arg("--no-first-run")
        .arg("--no-default-browser-check");
    if needs_no_sandbox() {
        config = config.arg("--no-sandbox");
    }
    let config = config
        .build()
        .map_err(|e| format!("printrender: browser config: {e}"))?;
    let (browser, mut handler) = chromiumoxide::Browser::launch(config)
        .await
        .map_err(|e| format!("printrender: launch browser: {e}"))?;
    // CDP 连接泵：常驻后台，进程生命周期内随浏览器共存亡。
    tokio::spawn(
        async move { while futures_util::StreamExt::next(&mut handler).await.is_some() {} },
    );
    let _ = BROWSER.set(browser);
    BROWSER
        .get()
        .ok_or_else(|| "printrender: browser init".into())
}

/// 本机 Chrome 的 CDP 渲染器（默认实现）。
pub struct ChromeRenderer;

impl Renderer for ChromeRenderer {
    async fn render_pdf(&self, opts: PrintOptions) -> Result<Vec<u8>, String> {
        use chromiumoxide::cdp::browser_protocol::page::PrintToPdfParams;

        if opts.html.is_empty() {
            return Err("printrender: empty html".into());
        }
        if opts.paper_width_mm <= 0.0 || opts.paper_height_mm <= 0.0 {
            return Err("printrender: paper size required (mm)".into());
        }

        let browser = browser().await?;
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| format!("printrender: new page: {e}"))?;
        // html 经 set_content 写入（document.write 同语义），等待 body 就绪
        // 后再给 300ms 布局沉降（与上游 WaitReady+Sleep 同拍）。
        page.set_content(&opts.html)
            .await
            .map_err(|e| format!("printrender: set content: {e}"))?;
        tokio::time::sleep(Duration::from_millis(300)).await;

        let mm_to_inch = |mm: f64| mm / 25.4;
        // 零边距 + 纸张毫米转英寸（PrintToPDF 以英寸为单位）。
        let params = PrintToPdfParams {
            landscape: Some(opts.landscape),
            print_background: Some(opts.background),
            display_header_footer: Some(false),
            paper_width: Some(mm_to_inch(opts.paper_width_mm)),
            paper_height: Some(mm_to_inch(opts.paper_height_mm)),
            margin_top: Some(0.0),
            margin_bottom: Some(0.0),
            margin_left: Some(0.0),
            margin_right: Some(0.0),
            ..Default::default()
        };
        let pdf = page
            .pdf(params)
            .await
            .map_err(|e| format!("printrender: print to pdf: {e}"))?;
        if pdf.is_empty() {
            return Err("printrender: empty pdf".into());
        }
        Ok(pdf)
    }
}

/// 用默认渲染器打印 PDF。
pub async fn render_pdf(opts: PrintOptions) -> Result<Vec<u8>, String> {
    ChromeRenderer.render_pdf(opts).await
}
