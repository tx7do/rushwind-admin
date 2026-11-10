//! AI Agent 图执行引擎（M1）——从上游 ai_agent_engine.go 1:1 移植。
//!
//! 图 DSL、节点语义与校验规则要点：
//!   - 上下文按节点 id 分命名空间（map[节点id]map[变量]字符串值）：knowledge 写
//!     context/hits、llm 写 output、start 写运行入参。模板 `{{节点id.变量}}` 与
//!     条件表达式都读这份上下文；
//!   - 条件表达式走受限求值器（比较 + contains/len——见 [`eval_agent_condition`]；
//!     上游用 expr-lang，其完整语法面在这里以受支持子集落地，超集报错）；
//!   - LLM 与知识检索是注入的函数（Deps），服务装配真实现，单测注入桩，
//!     引擎本体不触网。

use std::collections::BTreeMap;

use serde::Deserialize;

/// 单次运行的节点执行步数上限（校验已保证无环；条件表达式意外状况的兜底）。
const AGENT_MAX_STEPS: usize = 50;

/// 合法节点类型集。
fn is_known_node_type(t: &str) -> bool {
    matches!(
        t,
        "start" | "knowledge" | "llm" | "condition" | "end" | "http" | "script"
    )
}

/// 图节点。data 保持无类型 Value：五种节点各自的键在执行处按需取。
#[derive(Debug, Clone, Deserialize)]
pub struct AgentGraphNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

/// 图边。branch 仅 condition 出边使用（"true"/"false"）。
#[derive(Debug, Clone, Deserialize)]
pub struct AgentGraphEdge {
    #[serde(default)]
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub branch: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentGraph {
    #[serde(default)]
    pub nodes: Vec<AgentGraphNode>,
    #[serde(default)]
    pub edges: Vec<AgentGraphEdge>,
}

/// 单节点/单次运行的 token 用量。
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct AgentUsage {
    pub prompt: u32,
    pub completion: u32,
    pub total: u32,
}

impl AgentUsage {
    fn add(&mut self, other: &AgentUsage) {
        self.prompt += other.prompt;
        self.completion += other.completion;
        self.total += other.total;
    }
}

/// 一步执行的轨迹记录（落 sys_ai_agent_runs.trace JSON 数组）。
#[derive(Debug, serde::Serialize)]
pub struct AgentTraceStep {
    #[serde(rename = "nodeId")]
    pub node_id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub status: String, // ok | error
    pub ms: i64,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<AgentUsage>,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub error: String,
}

/// 解析图 DSL（JSON → 结构）。
pub fn parse_agent_graph(raw: &str) -> Result<AgentGraph, String> {
    if raw.trim().is_empty() {
        return Err("graph is empty".into());
    }
    serde_json::from_str(raw).map_err(|e| format!("graph is not valid JSON: {e}"))
}

fn valid_node_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 64 {
        return false;
    }
    for (i, c) in id.chars().enumerate() {
        let ok = c.is_ascii_alphabetic() || c == '_' || (c.is_ascii_digit() && i > 0);
        if !ok {
            return false;
        }
    }
    true
}

/// Run 前的静态校验；规则清单与上游 §2.3 一致。
pub fn validate_agent_graph(g: &AgentGraph) -> Result<(), String> {
    if g.nodes.is_empty() {
        return Err("graph has no nodes".into());
    }

    let mut by_id: BTreeMap<&str, &AgentGraphNode> = BTreeMap::new();
    let (mut start_count, mut end_count) = (0usize, 0usize);
    for n in &g.nodes {
        if !is_known_node_type(&n.node_type) {
            return Err(format!(
                "node {:?} has unknown type {:?}",
                n.id, n.node_type
            ));
        }
        if !valid_node_id(&n.id) {
            return Err(format!(
                "node id {:?} is invalid (letters/digits/underscore, must not start with a digit)",
                n.id
            ));
        }
        if by_id.contains_key(n.id.as_str()) {
            return Err(format!("duplicate node id {:?}", n.id));
        }
        by_id.insert(n.id.as_str(), n);
        match n.node_type.as_str() {
            "start" => start_count += 1,
            "end" => end_count += 1,
            "http" => validate_agent_http_node(n)?,
            "script" => validate_agent_script_node(n)?,
            _ => {}
        }
    }
    if start_count != 1 {
        return Err(format!(
            "graph must have exactly one start node, got {start_count}"
        ));
    }
    if end_count == 0 {
        return Err("graph must have at least one end node".into());
    }

    let mut edge_seen = std::collections::HashSet::new();
    let mut out: BTreeMap<&str, Vec<&AgentGraphEdge>> = BTreeMap::new();
    for e in &g.edges {
        if !by_id.contains_key(e.source.as_str()) {
            return Err(format!(
                "edge {:?} references unknown source node {:?}",
                e.id, e.source
            ));
        }
        if !by_id.contains_key(e.target.as_str()) {
            return Err(format!(
                "edge {:?} references unknown target node {:?}",
                e.id, e.target
            ));
        }
        let key = format!("{}->{}", e.source, e.target);
        if !edge_seen.insert(key.clone()) {
            return Err(format!("duplicate edge {key}"));
        }
        out.entry(e.source.as_str()).or_default().push(e);
    }

    for n in &g.nodes {
        let edges = out.get(n.id.as_str()).map(Vec::as_slice).unwrap_or(&[]);
        match n.node_type.as_str() {
            "end" => {
                if !edges.is_empty() {
                    return Err(format!("end node {:?} must not have outgoing edges", n.id));
                }
            }
            "condition" => {
                if edges.len() != 2 {
                    return Err(format!(
                        "condition node {:?} must have exactly two outgoing edges, got {}",
                        n.id,
                        edges.len()
                    ));
                }
                let mut branches = std::collections::HashSet::new();
                for e in edges {
                    if e.branch != "true" && e.branch != "false" {
                        return Err(format!(
                            "condition node {:?} outgoing edge must carry branch=true|false",
                            n.id
                        ));
                    }
                    branches.insert(e.branch.as_str());
                }
                if !branches.contains("true") || !branches.contains("false") {
                    return Err(format!(
                        "condition node {:?} must have one true and one false branch",
                        n.id
                    ));
                }
            }
            _ => {
                if edges.len() > 1 {
                    return Err(format!(
                        "node {:?} ({}) must have at most one outgoing edge",
                        n.id, n.node_type
                    ));
                }
            }
        }
    }

    // 可达性：从 start 出发沿全部边走，所有节点必须可达。
    let start = by_id.values().find(|n| n.node_type == "start").unwrap();
    let mut reachable = std::collections::HashSet::new();
    fn visit(
        id: &str,
        out: &BTreeMap<&str, Vec<&AgentGraphEdge>>,
        reachable: &mut std::collections::HashSet<String>,
    ) {
        if !reachable.insert(id.to_string()) {
            return;
        }
        if let Some(edges) = out.get(id) {
            for e in edges {
                visit(e.target.as_str(), out, reachable);
            }
        }
    }
    if let Some(edges) = out.get(start.id.as_str()) {
        for e in edges {
            visit(e.target.as_str(), &out, &mut reachable);
        }
    }
    reachable.insert(start.id.clone());
    for n in &g.nodes {
        if !reachable.contains(&n.id) {
            return Err(format!("node {:?} is not reachable from start", n.id));
        }
    }
    Ok(())
}

/// data.map 的字符串取值。
fn data_string(data: &serde_json::Value, key: &str) -> String {
    data.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// data.map 的 u32 取值（JSON 数字或数字字符串）。
#[cfg(test)]
pub fn data_u32(data: &serde_json::Value, key: &str) -> u32 {
    match data.get(key) {
        Some(serde_json::Value::Number(n)) => n.as_u64().filter(|v| *v > 0).unwrap_or(0) as u32,
        Some(serde_json::Value::String(s)) => s.trim().parse::<u32>().unwrap_or(0),
        _ => 0,
    }
}

/// data.map 的 u64 取值（JSON 数字或数字字符串）——节点里的实体 ID
/// （providerId/baseId）随契约批量扩宽走 u64。
pub fn data_u64(data: &serde_json::Value, key: &str) -> u64 {
    match data.get(key) {
        Some(serde_json::Value::Number(n)) => n.as_u64().filter(|v| *v > 0).unwrap_or(0),
        Some(serde_json::Value::String(s)) => s.trim().parse::<u64>().unwrap_or(0),
        _ => 0,
    }
}

pub fn data_i64(data: &serde_json::Value, key: &str, def: i64) -> i64 {
    match data.get(key) {
        Some(serde_json::Value::Number(n)) => n.as_i64().filter(|v| *v > 0).unwrap_or(def),
        _ => def,
    }
}

/// 上游 LLM 节点 temperature 覆盖的取值器；工具循环以单轮直调等价落地后
/// 该旋钮未接（llm.rs 的补全面无 temperature 参数），随多轮工具里程碑启用。
#[allow(dead_code)]
fn data_f64(data: &serde_json::Value, key: &str) -> Option<f64> {
    data.get(key).and_then(|v| v.as_f64())
}

/// data.map 的对象数组取值（inputVars 用）。
fn data_objects(data: &serde_json::Value, key: &str) -> Vec<serde_json::Value> {
    data.get(key)
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

/// 单趟替换 `{{node.var}}`（每个值只被消费一次；未知占位符报错——拼错的名字
/// 悄悄变空串，用户只会看到"模型没收到资料"，排障方向整个是反的）。
fn render_agent_template(
    tpl: &str,
    vars: &BTreeMap<String, BTreeMap<String, String>>,
) -> Result<String, String> {
    let mut out = String::with_capacity(tpl.len());
    let mut rest = tpl;
    while !rest.is_empty() {
        let Some(open) = rest.find("{{") else {
            out.push_str(rest);
            break;
        };
        let after_open = &rest[open + 2..];
        let Some(closing) = after_open.find("}}") else {
            out.push_str(rest);
            break;
        };
        let name = after_open[..closing].trim();
        let Some(dot) = name.find('.') else {
            return Err(format!(
                "template uses invalid placeholder {{{{{name}}}}} (expected {{{{node.var}}}})"
            ));
        };
        if dot == 0 || dot == name.len() - 1 {
            return Err(format!(
                "template uses invalid placeholder {{{{{name}}}}} (expected {{{{node.var}}}})"
            ));
        }
        let (node_id, var_name) = (&name[..dot], &name[dot + 1..]);
        let Some(ns) = vars.get(node_id) else {
            return Err(format!("template references unknown node {node_id:?}"));
        };
        let Some(value) = ns.get(var_name) else {
            return Err(format!(
                "template references unknown variable {var_name:?} of node {node_id:?}"
            ));
        };
        out.push_str(&rest[..open]);
        out.push_str(value);
        rest = &after_open[closing + 2..];
    }
    Ok(out)
}

/// 受限条件求值器（上游 expr-lang 的常用子集）：
///   - 比较式 `A op B`（== != > < >= <=；操作数为带引号字符串、数字或 `node.var` 引用）；
///   - `contains(A, B)`（字符串包含）与 `len(A) op N`（命名空间值长度比较）。
///   - 超出语法的表达式报错（而非默认 false）——静默 false 会把分支走向变成玄学。
fn eval_agent_condition(
    expression: &str,
    vars: &BTreeMap<String, BTreeMap<String, String>>,
) -> Result<bool, String> {
    let expr_text = expression.trim();
    if expr_text.is_empty() {
        return Err("condition expression is empty".into());
    }
    let resolve = |tok: &str| -> Result<String, String> {
        let tok = tok.trim();
        if (tok.starts_with('"') && tok.ends_with('"') && tok.len() >= 2)
            || (tok.starts_with('\'') && tok.ends_with('\'') && tok.len() >= 2)
        {
            return Ok(tok[1..tok.len() - 1].to_string());
        }
        if let Ok(n) = tok.parse::<f64>() {
            return Ok(format_token_number(n));
        }
        let Some(dot) = tok.find('.') else {
            return Err(format!(
                "condition operand {tok:?} is not a literal or node.var reference"
            ));
        };
        let (node_id, var_name) = (tok[..dot].trim(), tok[dot + 1..].trim());
        vars.get(node_id)
            .and_then(|ns| ns.get(var_name))
            .cloned()
            .ok_or_else(|| format!("condition references unknown {tok:?}"))
    };
    let num_of = |s: &str| s.trim().parse::<f64>().ok();

    // contains(A, B)
    if let Some(rest) = expr_text
        .strip_prefix("contains(")
        .and_then(|r| r.strip_suffix(')'))
    {
        let (a, b) = split_two_args(rest)?;
        let (a, b) = (resolve(&a)?, resolve(&b)?);
        return Ok(a.contains(&b));
    }
    // len(A) op N
    if let Some(rest) = expr_text.strip_prefix("len(") {
        let close = rest.find(')').ok_or("condition len( is not closed")?;
        let inner = &rest[..close];
        let tail = rest[close + 1..].trim();
        let (lhs, op) = (resolve(inner)?, parse_compare_op(tail)?);
        let rhs_text = tail[op.len()..].trim();
        let rhs = rhs_text
            .parse::<f64>()
            .map_err(|_| format!("condition len rhs {rhs_text:?} is not a number"))?;
        let l = lhs.chars().count() as f64;
        return Ok(apply_compare(l, op, rhs));
    }
    // A op B
    for op in ["==", "!=", ">=", "<=", ">", "<"] {
        if let Some(idx) = find_top_level(expr_text, op) {
            let lhs = resolve(&expr_text[..idx])?;
            let rhs = resolve(&expr_text[idx + op.len()..])?;
            return Ok(match op {
                "==" => lhs == rhs,
                "!=" => lhs != rhs,
                _ => {
                    let (a, b) = (
                        num_of(&lhs)
                            .ok_or_else(|| format!("condition lhs {lhs:?} is not numeric"))?,
                        num_of(&rhs)
                            .ok_or_else(|| format!("condition rhs {rhs:?} is not numeric"))?,
                    );
                    apply_compare(a, op, b)
                }
            });
        }
    }
    Err(format!(
        "condition expression {expr_text:?} is outside the supported subset (compare / contains / len)"
    ))
}

fn format_token_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn split_two_args(rest: &str) -> Result<(String, String), String> {
    let mut depth = 0i32;
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                return Ok((rest[..i].to_string(), rest[i + 1..].to_string()));
            }
            _ => {}
        }
    }
    Err("condition function expects two arguments".into())
}

fn find_top_level(text: &str, op: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'"' | b'\'' => {
                // 跳过字符串字面量
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
            }
            _ if depth == 0
                && text[i..].starts_with(op)
                && (op.len() > 1
                    || !matches!(bytes.get(i + 1), Some(b'=') | Some(b'>') | Some(b'<'))) =>
            {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_compare_op(tail: &str) -> Result<&'static str, String> {
    for op in [">=", "<=", "==", "!=", ">", "<"] {
        if tail.starts_with(op) {
            return Ok(op);
        }
    }
    Err(format!(
        "condition tail {tail:?} carries no comparison operator"
    ))
}

fn apply_compare(a: f64, op: &str, b: f64) -> bool {
    match op {
        ">" => a > b,
        "<" => a < b,
        ">=" => a >= b,
        "<=" => a <= b,
        _ => false,
    }
}

/// 检索节点外呼：节点定义 + 渲染后的查询 →（拼好的上下文, 命中条数）。
pub type SearchKnowledgeFn<'a> =
    Box<dyn Fn(&AgentGraphNode, &str) -> Result<(String, usize), String> + Send + 'a>;
/// LLM 节点外呼：节点定义 + system/user 提示 →（答案, 用量）。
pub type RunLlmFn<'a> =
    Box<dyn Fn(&AgentGraphNode, &str, &str) -> Result<(String, AgentUsage), String> + Send + 'a>;

/// script 节点外呼：节点定义 + 全图上下文 → 脚本产出（键值字符串命名空间）。
pub type RunScriptFn<'a> = Box<
    dyn Fn(
            &AgentGraphNode,
            &BTreeMap<String, BTreeMap<String, String>>,
        ) -> Result<BTreeMap<String, String>, String>
        + Send
        + 'a,
>;

/// 引擎的外呼依赖（服务装配真实现；测试注入桩）。
pub struct Deps<'a> {
    /// 执行知识检索节点，返回拼好的上下文与命中条数。
    pub search_knowledge: SearchKnowledgeFn<'a>,
    /// 执行 LLM 节点，返回最终答案与用量。
    pub run_llm: RunLlmFn<'a>,
    /// 执行 script 节点：一次性隔离沙箱跑完返回产出。nil（None）＝script
    /// 节点不可用并在执行处报错（上游 scriptRuntime 未装配同语义）。
    pub run_script: Option<RunScriptFn<'a>>,
}

/// 从 start 走图到 end，返回产出、轨迹与累计用量；任一节点执行失败即返回
/// 错误（trace 保留到失败步为止，由调用方落 FAILED 运行行）。
#[derive(Debug)]
pub struct Execution {
    pub output: String,
    pub trace: Vec<AgentTraceStep>,
    pub usage: AgentUsage,
}

pub fn execute_agent_graph(
    g: &AgentGraph,
    input: &BTreeMap<String, String>,
    deps: &Deps<'_>,
) -> Result<Execution, String> {
    let mut by_id: BTreeMap<&str, &AgentGraphNode> = BTreeMap::new();
    let mut out: BTreeMap<&str, Vec<&AgentGraphEdge>> = BTreeMap::new();
    for n in &g.nodes {
        by_id.insert(n.id.as_str(), n);
    }
    for e in &g.edges {
        out.entry(e.source.as_str()).or_default().push(e);
    }

    let start = *by_id
        .values()
        .find(|n| n.node_type == "start")
        .ok_or("graph has no start node")?;
    // 命名空间：节点 id → 变量集。start 的输入变量按声明必填校验。
    let mut vars: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for raw in data_objects(&start.data, "inputVars") {
        let name = raw.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let required = raw
            .get("required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if required && !input.contains_key(name) {
            return Err(format!("missing required input variable {name:?}"));
        }
    }
    vars.insert(start.id.clone(), input.clone());

    let mut trace: Vec<AgentTraceStep> = Vec::new();
    let mut usage = AgentUsage::default();
    let mut current = start;

    for _step in 0..AGENT_MAX_STEPS {
        let step_start = std::time::Instant::now();
        let node = current;
        let mut step_usage = AgentUsage::default();
        let summary: String;
        let branch_taken; // condition 节点的分支判定，选边用

        match node.node_type.as_str() {
            "start" => {
                summary = format!(
                    "{} input variable(s)",
                    vars.get(&start.id).map(|m| m.len()).unwrap_or(0)
                );
                branch_taken = "";
            }
            "knowledge" => {
                let query = render_agent_template(&data_string(&node.data, "query"), &vars)
                    .map_err(|e| step_error(node, &e))?;
                let (context_text, hits) =
                    (deps.search_knowledge)(node, &query).map_err(|e| step_error(node, &e))?;
                vars.insert(
                    node.id.clone(),
                    BTreeMap::from([
                        ("context".into(), context_text.clone()),
                        ("hits".into(), hits.to_string()),
                    ]),
                );
                summary = format!("hits={hits}, {} chars", context_text.len());
                branch_taken = "";
            }
            "llm" => {
                let system = render_agent_template(&data_string(&node.data, "systemPrompt"), &vars)
                    .map_err(|e| step_error(node, &e))?;
                let user = render_agent_template(&data_string(&node.data, "userPrompt"), &vars)
                    .map_err(|e| step_error(node, &e))?;
                if user.trim().is_empty() {
                    return Err(step_error(
                        node,
                        &format!(
                            "llm node {:?} has empty userPrompt after rendering",
                            node.id
                        ),
                    ));
                }
                let (answer, llm_usage) =
                    (deps.run_llm)(node, &system, &user).map_err(|e| step_error(node, &e))?;
                step_usage = llm_usage;
                usage.add(&llm_usage);
                vars.insert(
                    node.id.clone(),
                    BTreeMap::from([("output".into(), answer.clone())]),
                );
                summary = truncate_runes(&answer, 120);
                branch_taken = "";
            }
            "http" => {
                let (status, body, content_type) = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(run_agent_http_node(node, &vars))
                })
                .map_err(|e| step_error(node, &e))?;
                vars.insert(
                    node.id.clone(),
                    BTreeMap::from([
                        ("status".into(), status.clone()),
                        ("body".into(), body.clone()),
                        ("contentType".into(), content_type),
                    ]),
                );
                summary = format!("status={status}, {} bytes", body.len());
                branch_taken = "";
            }
            "script" => {
                // source 是代码、不是模板——代码里的 {{...}} 是脚本自身语法，
                // 绝不能过 render_agent_template（与 http 节点"字段值过模板"的关键差异）。
                let Some(run_script) = &deps.run_script else {
                    return Err(step_error(
                        node,
                        &format!(
                            "script node {:?} unavailable: script runtime not wired",
                            node.id
                        ),
                    ));
                };
                let outs = run_script(node, &vars).map_err(|e| step_error(node, &e))?;
                let mut keys: Vec<&String> = outs.keys().collect();
                keys.sort();
                summary = truncate_runes(
                    &format!(
                        "keys={}",
                        keys.iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                    120,
                );
                vars.insert(node.id.clone(), outs);
                branch_taken = "";
            }
            "condition" => {
                let approved = eval_agent_condition(&data_string(&node.data, "expression"), &vars)
                    .map_err(|e| step_error(node, &e))?;
                branch_taken = if approved { "true" } else { "false" };
                summary = format!("branch={approved}");
            }
            "end" => {
                let out_text = render_agent_template(&data_string(&node.data, "output"), &vars)
                    .map_err(|e| step_error(node, &e))?;
                trace.push(AgentTraceStep {
                    node_id: node.id.clone(),
                    node_type: node.node_type.clone(),
                    status: "ok".into(),
                    ms: step_start.elapsed().as_millis() as i64,
                    summary: "done".into(),
                    usage: None,
                    error: String::new(),
                });
                return Ok(Execution {
                    output: out_text,
                    trace,
                    usage,
                });
            }
            other => {
                return Err(format!("node {:?} has unknown type {:?}", node.id, other));
            }
        }

        trace.push(AgentTraceStep {
            node_id: node.id.clone(),
            node_type: node.node_type.clone(),
            status: "ok".into(),
            ms: step_start.elapsed().as_millis() as i64,
            summary,
            usage: (step_usage.total > 0).then_some(step_usage),
            error: String::new(),
        });

        // 选下一条边：condition 按 true/false 分支取，其余取唯一出边。
        let edges = out.get(node.id.as_str()).map(Vec::as_slice).unwrap_or(&[]);
        let next = edges
            .iter()
            .find(|e| node.node_type != "condition" || e.branch == branch_taken)
            .ok_or_else(|| format!("node {:?} has no outgoing edge to continue", node.id))?;
        current = *by_id
            .get(next.target.as_str())
            .ok_or_else(|| format!("edge target {:?} vanished", next.target))?;
    }
    Err(format!("execution exceeded {AGENT_MAX_STEPS} steps"))
}

fn step_error(node: &AgentGraphNode, err: &str) -> String {
    format!("node {}[{}]: {err}", node.id, node.node_type)
}

fn truncate_runes(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

// —— http 节点（docs/ai_agent_orchestration.md §2.1 节点集扩展，M2）——
// 渲染 URL/headers/body → SSRF 校验（与 RenderPdf 同款私网/回环/元数据防线，
// 判定面在 [`super::net_guard`]）→ 出站请求 → status/body/contentType 写入
// 节点上下文。data 键：url(必填,模板)、method(默认 GET，白名单
// GET/POST/PUT/DELETE/PATCH/HEAD)、headers(map 或 [{key,value}] 双形态，键值
// 均可模板)、body(模板，POST/PUT/PATCH 有意义)、timeoutSeconds(默认 15，钳
// [1,60])。

/// 响应体读取上限（防巨型响应撑爆内存与上下文）。
const AGENT_HTTP_MAX_BODY: usize = 64 << 10;

/// 写入节点上下文的响应体上限——上下文会进 LLM 提示词模板，超大 body 对
/// 下游模型是负担；超限截断并标注。
const AGENT_HTTP_MAX_CONTEXT_BODY: usize = 8 << 10;

/// 出站方法白名单：编排图的 HTTP 节点是"取数/回调"语义，不放行
/// CONNECT/TRACE/OPTIONS 等不用于此语义的方法。
fn agent_http_allowed_method(m: &str) -> bool {
    matches!(m, "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD")
}

/// Run 前静态校验（模板在执行期才渲染，这里只校验字面结构）。
fn validate_agent_http_node(n: &AgentGraphNode) -> Result<(), String> {
    if data_string(&n.data, "url").trim().is_empty() {
        return Err(format!("http node {:?} missing url", n.id));
    }
    let method = agent_http_method(n);
    if !agent_http_allowed_method(&method) {
        return Err(format!(
            "http node {:?} has disallowed method {method:?}",
            n.id
        ));
    }
    agent_http_timeout(n)?;
    Ok(())
}

fn agent_http_method(n: &AgentGraphNode) -> String {
    let m = data_string(&n.data, "method").trim().to_ascii_uppercase();
    if m.is_empty() {
        "GET".to_string()
    } else {
        m
    }
}

fn agent_http_timeout(n: &AgentGraphNode) -> Result<u64, String> {
    let raw = data_string(&n.data, "timeoutSeconds");
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(15);
    }
    let secs: u64 = raw
        .parse()
        .map_err(|_| format!("timeoutSeconds must be an integer in [1,60], got {raw:?}"))?;
    if !(1..=60).contains(&secs) {
        return Err(format!(
            "timeoutSeconds must be an integer in [1,60], got {raw:?}"
        ));
    }
    Ok(secs)
}

/// 渲染 → 校验 → 出站 → (status, body, contentType)。SSRF 防线与 RenderPdf
/// 同源；重定向跟随由 HTTP 客户端默认行为承接（与上游 net/http 一致，重定向
/// 后不复核——同一权衡）。
async fn run_agent_http_node(
    node: &AgentGraphNode,
    vars: &BTreeMap<String, BTreeMap<String, String>>,
) -> Result<(String, String, String), String> {
    let raw_url = render_agent_template(&data_string(&node.data, "url"), vars)?;
    let raw_url = raw_url.trim();
    super::net_guard::validate_outbound_url(raw_url)?;

    let method = agent_http_method(node);
    let timeout = agent_http_timeout(node)?;

    let payload = if method != "GET" && method != "HEAD" {
        render_agent_template(&data_string(&node.data, "body"), vars)?
    } else {
        String::new()
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()
        .map_err(|e| format!("build http client: {e}"))?;
    let method_kv = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|_| format!("http node {:?}: invalid method {method:?}", node.id))?;
    let mut req = client.request(method_kv, raw_url);
    req = req.header("User-Agent", "rushwind-agent-node/1.0");
    if !payload.is_empty() {
        req = req
            .header("Content-Type", "application/json")
            .body(payload.clone());
    }
    // headers：键值都可模板；画布存 [{key,value}] 数组，DSL 手写可能是
    // map——两种形态都收。非法头名/头值 fail-closed 报错。
    let header_err = |k: &str, e: String| format!("http node {:?}: header {k:?}: {e}", node.id);
    match node.data.get("headers") {
        Some(serde_json::Value::Object(map)) => {
            for (k, v) in map {
                let val = render_agent_template(&value_to_string(v), vars)?;
                req = req.header(
                    reqwest::header::HeaderName::from_bytes(k.as_bytes())
                        .map_err(|e| header_err(k, e.to_string()))?,
                    reqwest::header::HeaderValue::from_str(&val)
                        .map_err(|e| header_err(k, e.to_string()))?,
                );
            }
        }
        Some(serde_json::Value::Array(items)) => {
            for item in items {
                let Some(m) = item.as_object() else { continue };
                let k = m.get("key").and_then(|v| v.as_str()).unwrap_or("");
                if k.trim().is_empty() {
                    continue;
                }
                let val = render_agent_template(
                    &m.get("value").map(value_to_string).unwrap_or_default(),
                    vars,
                )?;
                req = req.header(
                    reqwest::header::HeaderName::from_bytes(k.as_bytes())
                        .map_err(|e| header_err(k, e.to_string()))?,
                    reqwest::header::HeaderValue::from_str(&val)
                        .map_err(|e| header_err(k, e.to_string()))?,
                );
            }
        }
        _ => {}
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status().as_u16().to_string();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();

    // 有界读取：读到上限 +1 即停（防巨型响应撑爆内存），截断标注。
    let mut raw: Vec<u8> = Vec::new();
    let mut over = false;
    let mut resp = resp;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("read response: {e}"))?
    {
        raw.extend_from_slice(&chunk);
        if raw.len() > AGENT_HTTP_MAX_BODY {
            over = true;
            raw.truncate(AGENT_HTTP_MAX_BODY);
            break;
        }
    }
    let full_len = raw.len();
    let mut body = String::from_utf8_lossy(&raw).to_string();
    if over {
        body.push_str("…(truncated at 64KB)");
    }
    if body.len() > AGENT_HTTP_MAX_CONTEXT_BODY {
        // 上下文截断按字符边界回退，不劈开多字节字符
        let mut cut = AGENT_HTTP_MAX_CONTEXT_BODY;
        while cut > 0 && !body.is_char_boundary(cut) {
            cut -= 1;
        }
        body = format!("{}…(truncated, full length {})", &body[..cut], full_len);
    }

    Ok((status, body, content_type))
}

/// headers 的 map 形态里值可能是任意 JSON 标量——字符串化后交给模板渲染。
fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(nodes: &[(&str, &str)], edges: &[(&str, &str, &str)]) -> AgentGraph {
        serde_json::from_value(serde_json::json!({
            "nodes": nodes.iter().map(|(id, t)| serde_json::json!({
                "id": id, "type": t, "data": {}
            })).collect::<Vec<_>>(),
            "edges": edges.iter().map(|(s, t, b)| serde_json::json!({
                "source": s, "target": t, "branch": b
            })).collect::<Vec<_>>(),
        }))
        .unwrap()
    }

    #[test]
    fn validation_enforces_shape_rules() {
        // 正常最小图
        let g = graph(&[("n1", "start"), ("n2", "end")], &[("n1", "n2", "")]);
        assert!(validate_agent_graph(&g).is_ok());
        // start 缺失
        let g = graph(&[("n2", "end")], &[]);
        assert!(validate_agent_graph(&g).unwrap_err().contains("start"));
        // end 缺失
        let g = graph(&[("n1", "start")], &[]);
        assert!(validate_agent_graph(&g).unwrap_err().contains("end node"));
        // condition 出边必须带 branch
        let g = graph(
            &[
                ("n1", "start"),
                ("n2", "condition"),
                ("n3", "end"),
                ("n4", "end"),
            ],
            &[("n1", "n2", ""), ("n2", "n3", ""), ("n2", "n4", "")],
        );
        assert!(validate_agent_graph(&g).unwrap_err().contains("branch"));
        // id 非法（数字开头）
        let g = graph(&[("1n", "start"), ("n2", "end")], &[("1n", "n2", "")]);
        assert!(validate_agent_graph(&g).unwrap_err().contains("invalid"));
        // 不可达节点
        let g = graph(
            &[("n1", "start"), ("n2", "end"), ("n3", "end")],
            &[("n1", "n2", "")],
        );
        assert!(validate_agent_graph(&g)
            .unwrap_err()
            .contains("not reachable"));
    }

    #[test]
    fn template_replaces_namespaced_placeholders() {
        let mut vars = BTreeMap::new();
        vars.insert(
            "n1".to_string(),
            BTreeMap::from([("output".to_string(), "答案".to_string())]),
        );
        assert_eq!(
            render_agent_template("结果：{{n1.output}}", &vars).unwrap(),
            "结果：答案"
        );
        // 未知节点 / 缺点号 / 未知变量
        assert!(render_agent_template("{{nx.output}}", &vars).is_err());
        assert!(render_agent_template("{{n1output}}", &vars).is_err());
        assert!(render_agent_template("{{n1.other}}", &vars).is_err());
    }

    #[test]
    fn condition_subset_evaluates_and_rejects() {
        let mut vars = BTreeMap::new();
        vars.insert(
            "n1".to_string(),
            BTreeMap::from([
                ("output".to_string(), "通过".to_string()),
                ("hits".to_string(), "3".to_string()),
            ]),
        );
        assert!(eval_agent_condition("n1.output == \"通过\"", &vars).unwrap());
        assert!(!eval_agent_condition("n1.output == \"拒绝\"", &vars).unwrap());
        assert!(eval_agent_condition("n1.output != \"拒绝\"", &vars).unwrap());
        assert!(eval_agent_condition("contains(n1.output, \"通\")", &vars).unwrap());
        assert!(eval_agent_condition("len(n1.output) >= 1", &vars).unwrap());
        assert!(!eval_agent_condition("n1.hits > 5", &vars).unwrap());
        // 超出子集：函数调用/算术
        assert!(eval_agent_condition("n1.output + n1.hits > 1", &vars).is_err());
        assert!(eval_agent_condition("upper(n1.output) == \"X\"", &vars).is_err());
    }

    #[test]
    fn engine_walks_start_llm_end_with_usage_and_trace() {
        let g = serde_json::from_value(serde_json::json!({
            "nodes": [
                {"id": "s", "type": "start", "data": {"inputVars": [{"name": "q", "required": true}]}},
                {"id": "m", "type": "llm", "data": {"userPrompt": "问：{{s.q}}", "systemPrompt": "你是助手"}},
                {"id": "e", "type": "end", "data": {"output": "{{m.output}}"}}
            ],
            "edges": [
                {"source": "s", "target": "m"},
                {"source": "m", "target": "e"}
            ]
        }))
        .unwrap();
        validate_agent_graph(&g).unwrap();
        let deps = Deps {
            search_knowledge: Box::new(|_, _| Ok((String::new(), 0))),
            run_script: None,
            run_llm: Box::new(|_, _sys, user| {
                assert_eq!(user, "问：你好");
                Ok((
                    "回答".into(),
                    AgentUsage {
                        prompt: 10,
                        completion: 5,
                        total: 15,
                    },
                ))
            }),
        };
        let input = BTreeMap::from([("q".to_string(), "你好".to_string())]);
        let exec = execute_agent_graph(&g, &input, &deps).unwrap();
        assert_eq!(exec.output, "回答");
        assert_eq!(exec.usage.total, 15);
        assert_eq!(exec.trace.len(), 3);
        assert_eq!(exec.trace[2].summary, "done");
        // 缺必填输入变量
        assert!(execute_agent_graph(&g, &BTreeMap::new(), &deps)
            .unwrap_err()
            .contains("missing required input variable"));
    }

    #[test]
    fn engine_takes_condition_branches_by_expression() {
        // 图：start → llm → condition(true→end-ok / false→end-bad)
        let mk = |expression: &str| {
            serde_json::from_value(serde_json::json!({
                "nodes": [
                    {"id": "s", "type": "start", "data": {}},
                    {"id": "m", "type": "llm", "data": {"userPrompt": "hi"}},
                    {"id": "c", "type": "condition", "data": {"expression": expression}},
                    {"id": "ok", "type": "end", "data": {"output": "pass"}},
                    {"id": "bad", "type": "end", "data": {"output": "fail"}}
                ],
                "edges": [
                    {"source": "s", "target": "m"},
                    {"source": "m", "target": "c"},
                    {"source": "c", "target": "ok", "branch": "true"},
                    {"source": "c", "target": "bad", "branch": "false"}
                ]
            }))
            .unwrap()
        };
        let deps = Deps {
            search_knowledge: Box::new(|_, _| Ok((String::new(), 0))),
            run_script: None,
            run_llm: Box::new(|_, _, _| Ok(("通过".into(), AgentUsage::default()))),
        };
        // llm 输出"通过" → n1.output == "通过" 为真 → ok 支
        let g = mk("m.output == \"通过\"");
        validate_agent_graph(&g).unwrap();
        let exec = execute_agent_graph(&g, &BTreeMap::new(), &deps).unwrap();
        assert_eq!(exec.output, "pass");
        // 假分支：条件不成立 → bad 支
        let g = mk("m.output == \"拒绝\"");
        let exec = execute_agent_graph(&g, &BTreeMap::new(), &deps).unwrap();
        assert_eq!(exec.output, "fail");
    }

    #[test]
    fn engine_knowledge_node_writes_context_and_hits() {
        let g = serde_json::from_value(serde_json::json!({
            "nodes": [
                {"id": "s", "type": "start", "data": {}},
                {"id": "k", "type": "knowledge",
                 "data": {"baseId": "7", "topK": 3, "query": "找：{{s.q}}"}},
                {"id": "e", "type": "end", "data": {"output": "{{k.hits}}/{{k.context}}"}}
            ],
            "edges": [
                {"source": "s", "target": "k"},
                {"source": "k", "target": "e"}
            ]
        }))
        .unwrap();
        let deps = Deps {
            run_script: None,
            search_knowledge: Box::new(|node, query| {
                // data_u32 的字符串分支（"7"）与模板渲染（{{s.q}}→input 注入）
                // 在 good 图验证；baseId 缺失按真实现报错（bad 图走此分支）。
                let base_id = super::data_u32(&node.data, "baseId");
                if base_id == 0 {
                    return Err("knowledge node has no baseId configured".into());
                }
                assert_eq!(base_id, 7);
                assert_eq!(query, "找：登录异常");
                Ok(("片段A\n\n片段B".into(), 2))
            }),
            run_llm: Box::new(|_, _, _| Ok((String::new(), AgentUsage::default()))),
        };
        let input = BTreeMap::from([("q".to_string(), "登录异常".to_string())]);
        let exec = execute_agent_graph(&g, &input, &deps).unwrap();
        assert_eq!(exec.output, "2/片段A\n\n片段B");
        // baseId 缺失 → knowledge 节点报错
        let bad = serde_json::from_value(serde_json::json!({
            "nodes": [
                {"id": "s", "type": "start", "data": {}},
                {"id": "k", "type": "knowledge", "data": {}},
                {"id": "e", "type": "end", "data": {}}
            ],
            "edges": [{"source": "s", "target": "k"}, {"source": "k", "target": "e"}]
        }))
        .unwrap();
        let err = execute_agent_graph(&bad, &BTreeMap::new(), &deps).unwrap_err();
        assert!(err.contains("no baseId"), "{err}");
    }

    #[test]
    fn probe_data_u32_from_graph_json() {
        let g: AgentGraph = serde_json::from_value(serde_json::json!({
            "nodes": [
                {"id": "k", "type": "knowledge",
                 "data": {"baseId": "7", "topK": 3, "query": "找：{{s.q}}"}}
            ],
            "edges": []
        }))
        .unwrap();
        let d = &g.nodes[0].data;
        println!("probe data = {d}");
        assert_eq!(data_u32(d, "baseId"), 7, "probe: {d}");
    }

    #[test]
    fn data_u32_accepts_number_and_numeric_string() {
        assert_eq!(data_u32(&serde_json::json!({"k": 42}), "k"), 42);
        assert_eq!(data_u32(&serde_json::json!({"k": "42"}), "k"), 42);
        assert_eq!(data_u32(&serde_json::json!({"k": 0}), "k"), 0);
        assert_eq!(data_u32(&serde_json::json!({}), "k"), 0);
        assert_eq!(data_i64(&serde_json::json!({"k": 5}), "k", 4), 5);
        // 非正数走默认值（上游语义：>0 才采纳）
        assert_eq!(data_i64(&serde_json::json!({"k": 0}), "k", 4), 4);
    }

    #[test]
    fn http_node_static_validation() {
        let node = |data: serde_json::Value| AgentGraphNode {
            id: "http_1".into(),
            node_type: "http".into(),
            data,
        };
        // 缺 url 拒绝
        let err =
            validate_agent_http_node(&node(serde_json::json!({"method": "GET"}))).unwrap_err();
        assert!(err.contains("missing url"), "{err}");
        // 方法白名单外拒绝
        let err = validate_agent_http_node(&node(serde_json::json!({
            "url": "https://example.com", "method": "CONNECT"
        })))
        .unwrap_err();
        assert!(err.contains("disallowed method"), "{err}");
        // method 小写归一化放行；缺省 method/timeout 合法
        validate_agent_http_node(&node(serde_json::json!({
            "url": "https://example.com", "method": "post"
        })))
        .unwrap();
        validate_agent_http_node(&node(serde_json::json!({"url": "https://example.com"}))).unwrap();
        // timeout 越界 / 非数值拒绝
        for bad in ["120", "0", "abc"] {
            let err = validate_agent_http_node(&node(serde_json::json!({
                "url": "https://example.com", "timeoutSeconds": bad
            })))
            .unwrap_err();
            assert!(err.contains("timeoutSeconds"), "{err}");
        }
        // 数字形态的 timeoutSeconds（画布可能存数值而非字符串）
        validate_agent_http_node(&node(serde_json::json!({
            "url": "https://example.com", "timeoutSeconds": 30
        })))
        .unwrap();
    }

    #[test]
    fn http_node_registered_in_graph_validation() {
        // http 节点进入合法类型集：含 http 节点的图能过静态校验
        let g = AgentGraph {
            nodes: vec![
                AgentGraphNode {
                    id: "start_1".into(),
                    node_type: "start".into(),
                    data: serde_json::json!({}),
                },
                AgentGraphNode {
                    id: "http_1".into(),
                    node_type: "http".into(),
                    data: serde_json::json!({"url": "https://example.com/api"}),
                },
                AgentGraphNode {
                    id: "end_1".into(),
                    node_type: "end".into(),
                    data: serde_json::json!({"output": "{{http_1.status}}"}),
                },
            ],
            edges: vec![
                AgentGraphEdge {
                    id: "e1".into(),
                    source: "start_1".into(),
                    target: "http_1".into(),
                    branch: String::new(),
                },
                AgentGraphEdge {
                    id: "e2".into(),
                    source: "http_1".into(),
                    target: "end_1".into(),
                    branch: String::new(),
                },
            ],
        };
        validate_agent_graph(&g).expect("graph with http node should pass static validation");
    }

    #[test]
    fn http_node_context_truncation_is_char_safe() {
        // 8KB 上下文截断：多字节字符不劈开（截点回退到字符边界）
        let body = "汉".repeat(5000); // 15000 bytes > 8KB
        let mut cut = AGENT_HTTP_MAX_CONTEXT_BODY;
        while cut > 0 && !body.is_char_boundary(cut) {
            cut -= 1;
        }
        assert_eq!(cut % 3, 0, "汉 是三字节字符，边界必落在 3 的倍数上");
    }
}

// —— script 节点（docs/ai_agent_orchestration.md §2.1 节点集扩展）——
// 把全图上下文交给一次性隔离沙箱（脚本引擎，与脚本系统试运行同款依赖面）
// 执行，脚本返回的表合并写回节点上下文。
//
// data 键：language("lua"|"javascript")、source(脚本源码)。
// source 是代码、不是模板——代码里的 {{...}} 是脚本自身语法，
// 绝不能过 render_agent_template（与 http 节点"字段值过模板"的关键差异）。
// 超时沿用脚本引擎 VM 缺省 5s，节点不带 timeout 字段（TestRun 形态定死）。

/// 脚本源码上限：script 节点语义是"小段数据加工"，不是承载完整应用的载体；
/// 超限请在脚本系统建正式脚本（平台管理员面）。
const AGENT_SCRIPT_MAX_SOURCE: usize = 32 << 10;

/// 语言白名单：与脚本系统的两种引擎对齐。
fn agent_script_allowed_language(lang: &str) -> bool {
    matches!(lang, "lua" | "javascript")
}

/// Run 前静态校验（source 是代码，只校验字面结构）。
fn validate_agent_script_node(n: &AgentGraphNode) -> Result<(), String> {
    let lang = data_string(&n.data, "language").trim().to_ascii_lowercase();
    if !agent_script_allowed_language(&lang) {
        return Err(format!(
            "script node {:?} has unsupported language {lang:?} (lua|javascript)",
            n.id
        ));
    }
    let source = data_string(&n.data, "source");
    if source.trim().is_empty() {
        return Err(format!("script node {:?} has empty source", n.id));
    }
    if source.len() > AGENT_SCRIPT_MAX_SOURCE {
        return Err(format!(
            "script node {:?} source exceeds {AGENT_SCRIPT_MAX_SOURCE} bytes",
            n.id
        ));
    }
    Ok(())
}

#[cfg(test)]
mod script_node_tests {
    use super::*;

    #[test]
    fn script_node_static_validation() {
        let node = |data: serde_json::Value| AgentGraphNode {
            id: "script_1".into(),
            node_type: "script".into(),
            data,
        };
        // 语言白名单外拒绝
        let err = validate_agent_script_node(&node(serde_json::json!({
            "language": "python", "source": "print(1)"
        })))
        .unwrap_err();
        assert!(err.contains("unsupported language"), "{err}");
        // 空 source 拒绝
        let err =
            validate_agent_script_node(&node(serde_json::json!({"language": "lua"}))).unwrap_err();
        assert!(err.contains("empty source"), "{err}");
        // 超 32KB 拒绝
        let err = validate_agent_script_node(&node(serde_json::json!({
            "language": "lua", "source": "x".repeat(33 << 10)
        })))
        .unwrap_err();
        assert!(err.contains("exceeds"), "{err}");
        // 大小写归一放行 + 缺 language 视为空 → 拒
        validate_agent_script_node(&node(serde_json::json!({
            "language": "LUA", "source": "return {a=1}"
        })))
        .unwrap();
        assert!(validate_agent_script_node(&node(serde_json::json!({"source": "x"}))).is_err());
    }

    #[test]
    fn script_node_registered_in_graph_validation() {
        // 含 script 节点的图能过静态校验
        let g = AgentGraph {
            nodes: vec![
                AgentGraphNode {
                    id: "start_1".into(),
                    node_type: "start".into(),
                    data: serde_json::json!({}),
                },
                AgentGraphNode {
                    id: "script_1".into(),
                    node_type: "script".into(),
                    data: serde_json::json!({"language": "lua", "source": "return {a=1}"}),
                },
                AgentGraphNode {
                    id: "end_1".into(),
                    node_type: "end".into(),
                    data: serde_json::json!({"output": "{{script_1.a}}"}),
                },
            ],
            edges: vec![
                AgentGraphEdge {
                    id: "e1".into(),
                    source: "start_1".into(),
                    target: "script_1".into(),
                    branch: String::new(),
                },
                AgentGraphEdge {
                    id: "e2".into(),
                    source: "script_1".into(),
                    target: "end_1".into(),
                    branch: String::new(),
                },
            ],
        };
        validate_agent_graph(&g).expect("graph with script node should pass static validation");
    }

    #[test]
    fn script_node_unwired_runtime_errors_clearly() {
        // run_script 未装配（None）：执行到 script 节点报"runtime not wired"
        // 而不是静默跳过——图里摆了 script 节点却静默不执行，
        // 用户只会看到"输出是空的"（上游 nil-runtime 同语义）。
        let g = AgentGraph {
            nodes: vec![
                AgentGraphNode {
                    id: "start_1".into(),
                    node_type: "start".into(),
                    data: serde_json::json!({}),
                },
                AgentGraphNode {
                    id: "script_1".into(),
                    node_type: "script".into(),
                    data: serde_json::json!({"language": "lua", "source": "return {a=1}"}),
                },
                AgentGraphNode {
                    id: "end_1".into(),
                    node_type: "end".into(),
                    data: serde_json::json!({"output": "{{script_1.a}}"}),
                },
            ],
            edges: vec![
                AgentGraphEdge {
                    id: "e1".into(),
                    source: "start_1".into(),
                    target: "script_1".into(),
                    branch: String::new(),
                },
                AgentGraphEdge {
                    id: "e2".into(),
                    source: "script_1".into(),
                    target: "end_1".into(),
                    branch: String::new(),
                },
            ],
        };
        let input: BTreeMap<String, String> = BTreeMap::new();
        let deps = Deps {
            search_knowledge: Box::new(|_, _| Ok((String::new(), 0))),
            run_llm: Box::new(|_, _, _| Ok((String::new(), AgentUsage::default()))),
            run_script: None,
        };
        let err = execute_agent_graph(&g, &input, &deps).unwrap_err();
        assert!(err.contains("script runtime not wired"), "{err}");
    }
}
