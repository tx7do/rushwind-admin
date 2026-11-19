//! update_mask 白名单门——字段级更新语义（上游 go-crud
//! FilterByFieldMask 的 Rust 对位）。契约 35 个 Update 消息都带
//! update_mask；本模块 + `set_masked!` 宏把它落成统一实现：
//!
//! - 无掩码 → 全量提交语义（存量 if-let-Some，一字不差）；
//! - 有掩码 → 只有选中字段允许动：Some 写值、None 显式清列；
//! - 点路径（`meta.title`）让顶层段（`meta`）进门（`touches`），但只有
//!   字面写 `meta` 才算"选中整个域"（`selects_whole`）——jsonb 域借此
//!   区分"载荷缺 = 不动列"与"点路径合并域接管"（menu 对位实现）。
//!
//! 路径归一：protojson 的 FieldMask 送 snake 原名，前端 makeUpdateMask
//! 却可能送 camel 表单键——去下划线小写对表，两边收敛到同一列。

use std::collections::HashSet;

#[derive(Debug, Clone)]
pub(crate) struct FieldMask {
    /// None = 请求没带掩码（全量提交）；Some = 归一后的全路径白名单
    /// （点路径保留 dots；含空集 = 什么都不许动，AIP-134 的
    /// present-but-empty 严格读法）。
    paths: Option<HashSet<String>>,
}

impl FieldMask {
    pub(crate) fn from(mask: Option<&pbjson_types::FieldMask>) -> Self {
        Self {
            paths: mask.map(|m| {
                m.paths
                    .iter()
                    .map(|p| normalize(p))
                    .filter(|p| !p.is_empty())
                    .collect()
            }),
        }
    }

    /// 显式无掩码——create/sync 等非 PATCH 入口复用 apply 链时用。
    pub(crate) fn unmasked() -> Self {
        Self { paths: None }
    }

    /// 请求没带掩码（全量提交语义，None 载荷不动列）。
    pub(crate) fn unrestricted(&self) -> bool {
        self.paths.is_none()
    }

    /// 字段是否被掩码选中。无掩码恒真；点路径按顶层段算选中
    /// （`meta.title` 选中 `meta` 域）。收 snake 或 camel 列名。
    pub(crate) fn touches(&self, field: &str) -> bool {
        match &self.paths {
            None => true,
            Some(set) => {
                let want = normalize(field);
                set.iter().any(|p| p.split('.').next().unwrap_or(p) == want)
            }
        }
    }

    /// 字段的"整个域"是否被字面选中（无掩码真；`meta.title` 只选子段，
    /// 不算选中整个 `meta`）。载荷 None 时只有 selected-whole 才显式清列
    /// ——仅点路径选中 + 载荷缺 = "未携带"，列原样留给合并域。
    pub(crate) fn selects_whole(&self, field: &str) -> bool {
        match &self.paths {
            None => true,
            Some(set) => set.contains(&normalize(field)),
        }
    }
}

fn normalize(field: &str) -> String {
    field.trim().replace('_', "").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(paths: &[&str]) -> FieldMask {
        FieldMask::from(Some(&pbjson_types::FieldMask {
            paths: paths.iter().map(|s| s.to_string()).collect(),
        }))
    }

    #[test]
    fn absent_mask_is_unrestricted_and_touches_everything() {
        let m = FieldMask::from(None);
        assert!(m.unrestricted());
        assert!(m.touches("name"));
        assert!(m.touches("meta"));
        assert!(m.selects_whole("meta"));
    }

    /// present-but-empty = 什么都不许动（AIP-134 严格读法）：前端表单
    /// 零键时送出的空串掩码不该被当成"全放行"。
    #[test]
    fn present_empty_mask_admits_nothing() {
        let m = mask(&[]);
        assert!(!m.unrestricted());
        assert!(!m.touches("name"));
        assert!(!m.selects_whole("name"));
    }

    #[test]
    fn top_level_paths_gate_exact_fields() {
        let m = mask(&["name", "status"]);
        assert!(m.touches("name"));
        assert!(m.touches("status"));
        assert!(!m.touches("path"));
        assert!(!m.touches("meta"));
        assert!(m.selects_whole("name"));
    }

    /// 点路径让顶层段进门，但不算选中整个域：menu 的 meta 合并域靠
    /// 这个区分"载荷缺不动列"与"整列写/清"。
    #[test]
    fn dotted_paths_admit_top_level_but_not_whole() {
        let m = mask(&["meta.title", "name"]);
        assert!(m.touches("meta"));
        assert!(m.touches("name"));
        assert!(!m.selects_whole("meta"));
        assert!(m.selects_whole("name"));
        assert!(!m.touches("icon"));
    }

    /// snake 原名与 camel 表单键归一到同一列：protojson 送
    /// hide_in_menu、前端送 hideInMenu，命中一致。
    #[test]
    fn snake_and_camel_names_normalize_together() {
        let m = mask(&["hide_in_menu"]);
        assert!(m.touches("hideInMenu"));
        assert!(m.touches("hide_in_menu"));
        assert!(m.selects_whole("hideInMenu"));
        let m2 = mask(&["hideInMenu"]);
        assert!(m2.touches("hide_in_menu"));
        assert!(!m2.touches("hideChildrenInMenu"));
    }

    #[test]
    fn empty_and_whitespace_paths_are_dropped() {
        let m = mask(&["", "  ", "name"]);
        assert!(m.touches("name"));
        assert!(!m.touches(""));
    }
}
