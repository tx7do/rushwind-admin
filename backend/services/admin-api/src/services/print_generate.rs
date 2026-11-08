//! AI 生成模板（M4-1）的护栏：LLM 产出的 TemplateData 必须过本模块校验
//! 才允许返回给前端（前端预览确认后再走既有 Create 保存）。
//! 原则与问数护栏一致：fail-closed，发现不可接受的形态直接拒绝而不是静默
//! 改写；但几何/长度类"钳制后语义不变"的项按上限收敛。

/// AI 生成 payload 的字节数上限。生成面远小于渲染面（RenderPdf 8MiB）——
/// 一句话描述产不出几十万字段的合法模板，超限即模型失控或提示注入，直接拒绝。
const MAX_PAYLOAD: usize = 512 << 10;

/// 单个模板元素数上限（真实单据几十个元素封顶）。
const MAX_ELEMENTS: usize = 200;

/// 单模板 {field} 绑定引用数上限。
const MAX_FIELD_REFS: usize = 300;

/// JSON 嵌套深度上限（TemplateData 合法形态最深 4 层：根 → elements[i] →
/// options → margin 数值）。
const MAX_JSON_DEPTH: usize = 8;

/// AI 生成文本类值的字符上限（formatter/名称/说明）。
const MAX_TEXT: usize = 2000;

/// 几何数值的毫米上限（A3 横向约 420mm，放大余量防溢出画布）。
const MAX_MM: f64 = 2000.0;

/// 页边距钳制上限（毫米）。
const MAX_MARGIN: f64 = 50.0;

/// AI 生成元素类型白名单（hiprint 内核家族的常规类型，从严取用）：未知类型
/// 一律拒绝（fail-closed）——设计器里用户仍可手动加，生成面不提供任意元素
/// 类型的攻击面。html 类不在列：任意 HTML 走渲染清洗那条链更合适，AI 生成
/// 不开这个口子。
fn allowed_type(t: &str) -> bool {
    matches!(
        t,
        "text"
            | "longtext"
            | "image"
            | "barcode"
            | "qrcode"
            | "table"
            | "rect"
            | "oval"
            | "line"
            | "customize"
    )
}

fn allowed_paper(p: &str) -> bool {
    matches!(p, "A3" | "A4" | "A5" | "B4" | "B5")
}

/// TemplateData 顶层键白名单（对齐种子模板与内核 generateHtml 消费的形态）。
/// 生成中出现其他键 → 拒绝（不静默丢弃，避免模型塞入内核未来才消费的键时
/// 行为不可预期）。
fn allowed_top_key(k: &str) -> bool {
    matches!(
        k,
        "unit"
            | "paperSize"
            | "orientation"
            | "schemaVersion"
            | "margins"
            | "header"
            | "footer"
            | "watermark"
            | "guides"
            | "name"
            | "elements"
            | "panels" // 面板容器：panelCount>1 时的 panels 数组
    )
}

/// 元素级键白名单（种子样例：id/type/options/printElementType；tid 为
/// hiprint 面板分类键，内核样例携带）。
fn allowed_elem_key(k: &str) -> bool {
    matches!(k, "id" | "type" | "options" | "printElementType" | "tid")
}

/// {field} 绑定占位符的字段名收集：标识符（字母/下划线开头，可含数字），
/// 不支持路径/表达式——表达式是设计器里的能力，AI 生成面不放行。
fn collect_field_refs(formatter: &str, fields: &mut std::collections::BTreeSet<String>) {
    let bytes = formatter.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        // 字段名本体：字母/下划线开头（按字符而非字节，避免劈开多字节字符）
        let mut ch = formatter[j..].chars();
        match ch.next() {
            Some(c) if c.is_ascii_alphabetic() || c == '_' => j += c.len_utf8(),
            _ => {
                i += 1;
                continue;
            }
        }
        while let Some(c) = formatter[j..].chars().next() {
            if c.is_ascii_alphanumeric() || c == '_' {
                j += c.len_utf8();
            } else {
                break;
            }
        }
        if j < bytes.len() && bytes[j] == b'}' && j > i + 1 {
            fields.insert(formatter[i + 1..j].to_string());
            i = j + 1;
        } else {
            i += 1;
        }
    }
}

/// 数值钳到 [min,max]；非数值返回 min（几何缺失由内核按默认处理，不因模型
/// 漏字段而整体拒绝）。范围内保留原始数值形态（整数仍是整数，与上游
/// json.Number 原样回传同语义），钳制后输出浮点数。
fn clamp_mm(v: serde_json::Value, min: f64, max: f64) -> serde_json::Value {
    let f = match &v {
        serde_json::Value::Number(n) => n.as_f64().filter(|f| f.is_finite()).unwrap_or(min),
        _ => min,
    };
    let clamped = f.clamp(min, max);
    if matches!(v, serde_json::Value::Number(_)) && clamped == f {
        return v;
    }
    serde_json::Number::from_f64(clamped)
        .map(serde_json::Value::Number)
        .unwrap_or(serde_json::Value::Null)
}

/// 校验 AI 生成的 TemplateData：返回原样 JSON（仅做几何/文本钳制的重新
/// 序列化）与引用的 {field} 集合。任何结构性越界（未知顶层键/未知元素类型/
/// 超深嵌套/超限）直接报错。
pub fn sanitize_generated_template(raw: &str) -> Result<(String, Vec<String>), String> {
    if raw.len() > MAX_PAYLOAD {
        return Err("generated template too large".into());
    }
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("generated template is empty".into());
    }

    let mut root: serde_json::Map<String, serde_json::Value> = serde_json::from_str(raw)
        .map_err(|_| "generated template is not valid json".to_string())?;

    // 顶层键白名单
    for k in root.keys() {
        if !allowed_top_key(k) {
            return Err(format!(
                "generated template: unexpected top-level key {k:?}"
            ));
        }
    }
    check_json_depth(&serde_json::Value::Object(root.clone()), 1)?;

    // 必填语义钳制：unit 固定毫米（内核按 mm 布局，其他单位直接拒）
    match root.get("unit") {
        Some(serde_json::Value::String(u)) if u == "mm" => {}
        _ => return Err("generated template: unit must be \"mm\"".into()),
    }
    // 纸张/方向：缺失时补默认（提示词要求过，但 LLM 漏字段是常态，补齐无害）
    let paper = match root.get("paperSize") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => s.clone(),
        _ => "A4".to_string(),
    };
    if !allowed_paper(&paper) {
        return Err(format!(
            "generated template: unsupported paperSize {paper:?}"
        ));
    }
    root.insert("paperSize".into(), serde_json::Value::String(paper));
    let orient = match root.get("orientation") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => s.clone(),
        _ => "portrait".to_string(),
    };
    if orient != "portrait" && orient != "landscape" {
        return Err("generated template: orientation must be portrait/landscape".into());
    }
    root.insert("orientation".into(), serde_json::Value::String(orient));
    root.insert("schemaVersion".into(), serde_json::json!(1));

    clamp_margins(&mut root)?;
    for zone in ["header", "footer"] {
        if let Some(serde_json::Value::Object(z)) = root.get_mut(zone) {
            let h = z.get("height").cloned().unwrap_or(serde_json::Value::Null);
            z.insert("height".into(), clamp_mm(h, 0.0, MAX_MARGIN));
        }
    }
    if let Some(serde_json::Value::String(name)) = root.get_mut("name") {
        if name.chars().count() > 200 {
            *name = name.chars().take(200).collect();
        }
    }

    // 元素：数量上限 + 逐项白名单 + 几何钳制 + 占位符收集
    let mut fields = std::collections::BTreeSet::new();
    let mut empty = Vec::new();
    let elems = match root.get_mut("elements") {
        Some(serde_json::Value::Array(a)) => a,
        _ => &mut empty,
    };
    if elems.len() > MAX_ELEMENTS {
        return Err(format!(
            "generated template: too many elements ({} > {MAX_ELEMENTS})",
            elems.len()
        ));
    }
    for (i, e) in elems.iter_mut().enumerate() {
        let serde_json::Value::Object(em) = e else {
            return Err(format!("generated template: element {i} is not an object"));
        };
        for k in em.keys() {
            if !allowed_elem_key(k) {
                return Err(format!(
                    "generated template: element {i} has unexpected key {k:?}"
                ));
            }
        }
        let typ = match em.get("type") {
            Some(serde_json::Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        if !allowed_type(&typ) {
            return Err(format!(
                "generated template: element {i} has disallowed type {typ:?}"
            ));
        }
        // id：字符串且非空（内核以 id 做元素寻址），上限收紧防滥用
        let id_ok = match em.get("id") {
            Some(serde_json::Value::String(id)) => !id.trim().is_empty(),
            _ => false,
        };
        if !id_ok {
            return Err(format!("generated template: element {i} missing id"));
        }
        if let Some(serde_json::Value::String(id)) = em.get("id") {
            if id.chars().count() > 100 {
                let short: String = id.chars().take(100).collect();
                em.insert("id".into(), serde_json::Value::String(short));
            }
        }
        let Some(serde_json::Value::Object(opts)) = em.get_mut("options") else {
            return Err(format!("generated template: element {i} missing options"));
        };
        for g in ["left", "top", "width", "height"] {
            let v = opts.get(g).cloned().unwrap_or(serde_json::Value::Null);
            opts.insert(g.into(), clamp_mm(v, 0.0, MAX_MM));
        }
        if let Some(serde_json::Value::String(f)) = opts.get("formatter") {
            if f.chars().count() > MAX_TEXT {
                return Err(format!(
                    "generated template: element {i} formatter too long"
                ));
            }
            collect_field_refs(f, &mut fields);
        }
    }
    if fields.len() > MAX_FIELD_REFS {
        return Err(format!(
            "generated template: too many field refs ({} > {MAX_FIELD_REFS})",
            fields.len()
        ));
    }

    let clean = serde_json::to_string(&serde_json::Value::Object(root))
        .map_err(|_| "re-serialize generated template failed".to_string())?;
    // BTreeSet 迭代即字典序，与上游的稳定排序同序
    Ok((clean, fields.into_iter().collect()))
}

/// 递归检查嵌套深度（object/array 各算一层）。
fn check_json_depth(v: &serde_json::Value, depth: usize) -> Result<(), String> {
    if depth > MAX_JSON_DEPTH {
        return Err(format!(
            "generated template: nesting too deep (> {MAX_JSON_DEPTH})"
        ));
    }
    match v {
        serde_json::Value::Object(m) => {
            for vv in m.values() {
                check_json_depth(vv, depth + 1)?;
            }
        }
        serde_json::Value::Array(a) => {
            for vv in a {
                check_json_depth(vv, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 校验并钳制 margins（缺省补 10；非数值拒绝；负值钳 0；超 50mm 钳上限）。
fn clamp_margins(root: &mut serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    let Some(serde_json::Value::Object(m)) = root.get_mut("margins") else {
        root.insert(
            "margins".into(),
            serde_json::json!({"top": 10, "right": 10, "bottom": 10, "left": 10}),
        );
        return Ok(());
    };
    for side in ["top", "right", "bottom", "left"] {
        let present = m.get(side).cloned();
        let Some(v) = present else {
            m.insert(side.into(), serde_json::json!(10));
            continue;
        };
        let f = match v.as_f64().filter(|f| f.is_finite()) {
            Some(f) => f,
            None => {
                return Err(format!(
                    "generated template: margins.{side} is not a number"
                ));
            }
        };
        let clamped = if f < 0.0 {
            serde_json::json!(0) // 负边距无物理意义，与元素几何同款钳 0
        } else if f > MAX_MARGIN {
            serde_json::Number::from_f64(MAX_MARGIN)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        } else {
            v
        };
        m.insert(side.into(), clamped);
    }
    Ok(())
}

/// 从模型回复中剥出 JSON 主体：容忍 ```json 围栏与前后闲话（取第一个 { 到
/// 最后一个 } 之间的内容——TemplateData 是单对象，不存在"正文里合法包含裸
/// 大括号对"的歧义）。
pub fn extract_template_payload(raw: &str) -> String {
    let raw = raw.trim();
    let raw = raw.strip_prefix("```json").unwrap_or(raw);
    let raw = raw.strip_prefix("```").unwrap_or(raw);
    let raw = raw.strip_suffix("```").unwrap_or(raw).trim();
    match (raw.find('{'), raw.rfind('}')) {
        (Some(start), Some(end)) if end > start => raw[start..=end].to_string(),
        _ => raw.to_string(),
    }
}

/// 生成调用的系统提示词：TemplateData 的骨架与约束写死（单位毫米 / 元素
/// 类型白名单 / 几何以毫米计 / 文本绑定用 {field} 占位符），模型越界由
/// sanitize_generated_template 统一兜底拒绝。
pub fn build_system_prompt(
    description: &str,
    paper: &str,
    orient: &str,
    resource_key: &str,
) -> String {
    let mut sb = String::new();
    sb.push_str("你是单据打印模板生成引擎，输出 hiprint 风格的 TemplateData JSON（毫米布局）。\n");
    sb.push_str("输出规则：\n");
    sb.push_str("1. 只输出一个 JSON 对象，不要解释、不要 Markdown 代码块；\n");
    sb.push_str("2. 顶层键只用：unit(固定\"mm\")、paperSize、orientation、schemaVersion(固定 1)、margins、header、footer、watermark、guides、name、elements；\n");
    sb.push_str("3. elements 元素只用这些 type：text、longtext、image、barcode、qrcode、table、rect、oval、line；\n");
    sb.push_str("4. 每个元素必须有 id(唯一)、type、options；options 里 left/top/width/height 用毫米数值，文本内容放 options.formatter；\n");
    sb.push_str("5. 数据绑定用 {fieldName} 占位符（如 出库单号：{orderNo}），不要输出表达式、函数或 HTML；\n");
    sb.push_str(
        "6. 表格元素用 type:\"table\"，列定义放 options.columns（每列 {title,field,width}）；\n",
    );
    sb.push_str(
        "7. 布局要符合单据排版惯例：标题居上居中、字段行靠左、明细表在中部、合计/签收在底部。\n\n",
    );
    sb.push_str("用户对单据的描述：\n");
    sb.push_str(description);
    sb.push_str("\n\n");
    if !resource_key.is_empty() {
        sb.push_str(&format!(
            "（该模板将绑定业务资源 {resource_key}，占位符字段名尽量贴合该资源的常见字段语义。）\n\n"
        ));
    }
    let _ = (paper, orient); // 纸张/方向进 user 消息（与上游同构）
    sb
}

/// 日志用截断（原始模型输出可能很长）。按字节截（日志尺寸的本意），截点
/// 回退到字符边界避免劈开多字节字符。
pub fn truncate_for_log(s: &str) -> String {
    const MAX: usize = 2000;
    if s.len() <= MAX {
        return s.to_string();
    }
    let mut cut = MAX;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}...(truncated)", &s[..cut])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sanitize_ok(raw: &str) -> (String, Vec<String>) {
        sanitize_generated_template(raw).expect("sanitize should accept")
    }

    #[test]
    fn minimal_valid_template_passes_with_defaults() {
        let (clean, fields) = sanitize_ok(r#"{"unit":"mm","elements":[]}"#);
        let v: serde_json::Value = serde_json::from_str(&clean).unwrap();
        assert_eq!(v["unit"], "mm");
        assert_eq!(v["paperSize"], "A4"); // 缺省补默认
        assert_eq!(v["orientation"], "portrait");
        assert_eq!(v["schemaVersion"], 1);
        assert_eq!(v["margins"]["top"], 10); // 缺省补 10
        assert!(fields.is_empty());
    }

    #[test]
    fn structural_violations_rejected() {
        // 未知顶层键
        assert!(sanitize_generated_template(r#"{"unit":"mm","evil":1}"#).is_err());
        // unit 非 mm
        assert!(sanitize_generated_template(r#"{"unit":"px"}"#).is_err());
        // 纸张越界
        assert!(sanitize_generated_template(r#"{"unit":"mm","paperSize":"X11"}"#).is_err());
        // 方向越界
        assert!(sanitize_generated_template(r#"{"unit":"mm","orientation":"diagonal"}"#).is_err());
        // 非 JSON / 空
        assert!(sanitize_generated_template("not json").is_err());
        assert!(sanitize_generated_template("  ").is_err());
    }

    #[test]
    fn element_rules_enforced() {
        // 未知元素类型 fail-closed
        assert!(sanitize_generated_template(
            r#"{"unit":"mm","elements":[{"id":"a","type":"html","options":{}}]}"#
        )
        .is_err());
        // 元素级未知键
        assert!(sanitize_generated_template(
            r#"{"unit":"mm","elements":[{"id":"a","type":"text","options":{},"script":"x"}]}"#
        )
        .is_err());
        // 缺 id / 缺 options
        assert!(sanitize_generated_template(
            r#"{"unit":"mm","elements":[{"type":"text","options":{}}]}"#
        )
        .is_err());
        assert!(sanitize_generated_template(
            r#"{"unit":"mm","elements":[{"id":"a","type":"text"}]}"#
        )
        .is_err());
        // 合法元素过闸，几何钳制生效（超限收到 2000，缺失落 0.0）
        let (clean, _) = sanitize_ok(
            r#"{"unit":"mm","elements":[{"id":"t1","type":"text","options":{"left":9999,"width":-5,"formatter":"单号 {orderNo}"}}]}"#,
        );
        let v: serde_json::Value = serde_json::from_str(&clean).unwrap();
        assert_eq!(
            v["elements"][0]["options"]["left"],
            serde_json::json!(2000.0)
        );
        assert_eq!(v["elements"][0]["options"]["width"], serde_json::json!(0.0));
        assert_eq!(
            v["elements"][0]["options"]["height"],
            serde_json::json!(0.0)
        );
    }

    #[test]
    fn field_refs_collected_deduped_sorted() {
        let (_, fields) = sanitize_ok(
            r#"{"unit":"mm","elements":[
                {"id":"a","type":"text","options":{"formatter":"{b} {a} {b}"}},
                {"id":"c","type":"text","options":{"formatter":"{c10_x}"}}]}"#,
        );
        assert_eq!(fields, vec!["a", "b", "c10_x"]);
        // 表达式/路径形态不收集也不放行成字段
        let (_, fields) = sanitize_ok(
            r#"{"unit":"mm","elements":[{"id":"a","type":"text","options":{"formatter":"{1abc} {a-b} {}"}}]}"#,
        );
        assert!(fields.is_empty());
    }

    #[test]
    fn margins_and_limits_clamped() {
        // 非数值 margins 拒绝
        assert!(sanitize_generated_template(r#"{"unit":"mm","margins":{"top":"x"}}"#).is_err());
        // 负值钳 0、超限钳 50
        let (clean, _) = sanitize_ok(r#"{"unit":"mm","margins":{"top":-3,"bottom":500}}"#);
        let v: serde_json::Value = serde_json::from_str(&clean).unwrap();
        assert_eq!(v["margins"]["top"], 0);
        assert_eq!(v["margins"]["bottom"], serde_json::json!(50.0));
        assert_eq!(v["margins"]["left"], 10);
        // header/footer 高度钳 0..=50
        let (clean, _) =
            sanitize_ok(r#"{"unit":"mm","header":{"height":120},"footer":{"height":8}}"#);
        let v: serde_json::Value = serde_json::from_str(&clean).unwrap();
        assert_eq!(v["header"]["height"], serde_json::json!(50.0));
        // 范围内保留原值形态（整数仍是整数，json.Number 原样回传语义）
        assert_eq!(v["footer"]["height"], serde_json::json!(8));
    }

    #[test]
    fn payload_and_depth_caps_reject() {
        let big = format!(r#"{{"unit":"mm","name":"{}"}}"#, "x".repeat(600 << 10));
        assert!(sanitize_generated_template(&big).is_err());
        // 超深嵌套（> 8 层）
        let mut deep = String::from(r#"{"unit":"mm","guides":"#);
        for _ in 0..12 {
            deep.push('[');
        }
        for _ in 0..12 {
            deep.push(']');
        }
        deep.push('}');
        assert!(sanitize_generated_template(&deep).is_err());
    }

    #[test]
    fn extraction_tolerates_fences_and_chatter() {
        assert_eq!(
            extract_template_payload("```json\n{\"a\":1}\n```"),
            r#"{"a":1}"#
        );
        assert_eq!(
            extract_template_payload("好的，这是模板：{\"a\":{\"b\":2}} 请查收"),
            r#"{"a":{"b":2}}"#
        );
        assert_eq!(extract_template_payload("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn name_truncated_to_200_runes() {
        let long = "汉".repeat(300);
        let (clean, _) = sanitize_ok(&format!(r#"{{"unit":"mm","name":"{long}"}}"#));
        let v: serde_json::Value = serde_json::from_str(&clean).unwrap();
        assert_eq!(v["name"].as_str().unwrap().chars().count(), 200);
    }

    #[test]
    fn log_truncation_is_byte_bounded_and_char_safe() {
        let s = "汉".repeat(1500); // 4500 bytes
        let t = truncate_for_log(&s);
        assert!(t.ends_with("...(truncated)"));
        // 截点落在字符边界：尾部无半个多字节字符，字节总量受控
        assert!(t.len() <= 2000 + "...(truncated)".len());
        let ascii = "a".repeat(3000);
        let t = truncate_for_log(&ascii);
        assert_eq!(t.len(), 2000 + "...(truncated)".len());
        assert_eq!(truncate_for_log("short"), "short");
    }
}
