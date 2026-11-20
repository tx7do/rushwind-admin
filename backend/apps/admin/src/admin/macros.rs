//! Service-layer boilerplate macros shared across the per-service
//! files — and exported for the business modules: a downstream module
//! crate implements its services against the same helpers (see
//! `business/module-template` for the consumer face).

/// The Id-only `query_by` extraction: returns the id, or fails with the
/// shared `query_by required` rejection for every other shape. The path
/// is the request's nested `QueryBy` enum.
#[macro_export]
macro_rules! query_by_id {
    ($query_by:expr, $($seg:ident)::+) => {
        match $query_by {
            Some($($seg)::+::Id(id)) => id,
            _ => return Err($crate::state::status_error("BAD_REQUEST", "query_by required")),
        }
    };
}

/// Stamp the editor columns on an update path: `updated_by` moves to
/// the operator and `updated_at` to now — always together.
#[macro_export]
macro_rules! stamp_update {
    ($model:expr, $user:expr) => {
        $model.updated_by = sea_orm::Set(Some($user));
        $model.updated_at = sea_orm::Set(Some($crate::data::now()));
    };
}

/// create 路径的缺省列兜底：字段在载荷里缺席时补默认值。替代
/// `a.field.clone().unwrap().is_none()` 的写法——那个 unwrap 在字段
/// NotSet（`set_masked!(opt …)` 无掩码 + 载荷省略时的落点）上会 panic，
/// 恰好炸在"缺省即兜底"的本意路径上；本宏把 NotSet/Set(None)/
/// Unchanged(None) 都视同"缺席"，语义与原 is_none() 判断一致。
#[macro_export]
macro_rules! ensure_default {
    ($model:expr, $field:ident, $value:expr) => {
        if matches!(
            $model.$field,
            sea_orm::ActiveValue::NotSet
                | sea_orm::ActiveValue::Set(None)
                | sea_orm::ActiveValue::Unchanged(None)
        ) {
            $model.$field = sea_orm::Set(Some($value));
        }
    };
}

/// update_mask 白名单门下的字段写入（`fieldmask::FieldMask` 搭档，字段级
/// 更新语义的统一实现面）：
///
/// - `opt` 形态给可空列：掩码选中且载荷 Some → `Set(map(v))`；掩码选中且
///   载荷 None → 显式 `Set(None)` 清列（清值通道）；无掩码时 None 不动列
///   （存量全量提交语义不变）。
/// - `req` 形态给 NOT NULL 列：掩码选中且载荷 Some → `Set(map(v))`；
///   None 永不写（无可清语义）。
///
/// `$map` 把 proto 值换成列值（恒等 `v.clone()`、枚举映射、类型转换都行）；
/// 字段名默认取 `stringify!($field)`，列名与 wire 名不一致（如
/// `type_column` ← proto `type`）时用 `opt_named`/`req_named` 显式给名；
/// 掩码侧 snake/camel 归一对表。
#[macro_export]
macro_rules! set_masked {
    (opt $mask:expr, $model:expr, $field:ident, $value:expr, $map:expr) => {
        $crate::set_masked!(opt_named $mask, $model, $field, stringify!($field), $value, $map)
    };
    (opt_named $mask:expr, $model:expr, $field:ident, $name:expr, $value:expr, $map:expr) => {
        if $mask.touches($name) {
            match &$value {
                Some(v) => $model.$field = sea_orm::Set($map(v)),
                None => {
                    if !$mask.unrestricted() {
                        $model.$field = sea_orm::Set(None);
                    }
                }
            }
        }
    };
    (req $mask:expr, $model:expr, $field:ident, $value:expr, $map:expr) => {
        $crate::set_masked!(req_named $mask, $model, $field, stringify!($field), $value, $map)
    };
    (req_named $mask:expr, $model:expr, $field:ident, $name:expr, $value:expr, $map:expr) => {
        if $mask.touches($name) {
            if let Some(v) = &$value {
                $model.$field = sea_orm::Set($map(v));
            }
        }
    };
}
