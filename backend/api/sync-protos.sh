#!/usr/bin/env bash
# sync-protos.sh — 把 go-wind-admin 的 proto 契约同步进本仓 backend/api/protos/，
# 并生成/校验 MANIFEST.sha256。
#
#   backend/api/sync-protos.sh           # 同步（默认源：D:\GoProject\go-wind-admin\backend\api\protos，
#                                       #        可用 GOWIND_ADMIN_API_DIR 覆盖）+ 重建 MANIFEST
#   backend/api/sync-protos.sh --check   # CI 门：校验 api/protos 与 MANIFEST 一致（防手改），
#                                       #        并对比源仓（漂移报告，发现漂移时退出码 2）
#
# 字节归一化：UTF-8 BOM 剥除（源仓有 2 个文件带 BOM：protoc 容忍、protox 拒绝）。
# 复制与哈希两侧统一走同一归一化，漂移检测语义不受影响。
#
# 依据 docs/development-plan.md §7 Phase 0：proto 是唯一契约；本仓持有同步副本 +
# checksum 门，上游漂移必须在同步时显式接受（重新生成 MANIFEST 并走差分回归）。
#
# 再同步接受新 vintage 时，建议先跑契约演进检查（api/buf.yaml 工作区）：
#   cd backend/api && buf breaking --against "../../.git#branch=main,subdir=backend/api"
# 工作区对工作区比较，报告相对 main 基线的 breaking 差异；
# FILE 规则下仅【删除/改型】算破坏，纯新增不算。
set -euo pipefail

MODE="${1:-sync}"
HERE="$(cd "$(dirname "$0")" && pwd)"
PROTO_DIR="$HERE/protos"
MANIFEST="$HERE/MANIFEST.sha256"
SRC="${GOWIND_ADMIN_API_DIR:-/d/GoProject/go-wind-admin/backend/api/protos}"

# BOM 归一化输出（无 BOM 原样，有 BOM 剥头三字节）。
norm() {
  local f="$1" b
  b="$(head -c 3 "$f" 2>/dev/null | od -An -tx1 | tr -d ' \n')"
  if [[ "$b" == "efbbbf" ]]; then
    tail -c +4 "$f"
  else
    cat "$f"
  fi
}

# 对 <dir> 下全部 .proto 生成归一化 sha256 清单（稳定排序）。
manifest_for() {
  local dir="$1"
  (
    cd "$dir" || exit 1
    find . -type f -name '*.proto' -print0 | sort -z | while IFS= read -r -d '' f; do
      local rel="${f#./}"
      local h
      h="$(norm "$dir/$rel" | sha256sum | cut -d' ' -f1)"
      printf '%s  %s\n' "$h" "$rel"
    done
  )
}

case "$MODE" in
  sync)
    if [[ ! -d "$SRC" ]]; then
      echo "ERROR: proto source not found: $SRC (set GOWIND_ADMIN_API_DIR)" >&2
      exit 1
    fi
    rm -rf "$PROTO_DIR"
    mkdir -p "$PROTO_DIR"
    # 按归一化字节复制（BOM 剥除）。
    (
      cd "$SRC" || exit 1
      find . -type f -name '*.proto' -print0 | sort -z | while IFS= read -r -d '' f; do
        rel="${f#./}"
        mkdir -p "$PROTO_DIR/$(dirname "$rel")"
        norm "$SRC/$rel" > "$PROTO_DIR/$rel"
      done
    )
    find "$PROTO_DIR" -type d -exec chmod 755 {} + 2>/dev/null || true
    find "$PROTO_DIR" -type f -exec chmod 644 {} + 2>/dev/null || true
    manifest_for "$PROTO_DIR" > "$MANIFEST"
    echo "synced $(find "$PROTO_DIR" -name '*.proto' | wc -l) proto files (BOM-normalized) from $SRC"
    echo "MANIFEST.sha256 regenerated ($(wc -l < "$MANIFEST") entries)."
    ;;

  check|--check)
    if [[ ! -f "$MANIFEST" ]]; then
      echo "ERROR: MANIFEST.sha256 missing; run backend/api/sync-protos.sh first" >&2
      exit 1
    fi
    if ! manifest_for "$PROTO_DIR" | diff -q - "$MANIFEST" >/dev/null; then
      echo "FAIL: api/protos does not match MANIFEST.sha256 (tampered or unsynced edits)" >&2
      manifest_for "$PROTO_DIR" | diff - "$MANIFEST" | head -20 >&2 || true
      exit 1
    fi
    echo "OK: api/protos matches MANIFEST.sha256"
    if [[ -d "$SRC" ]]; then
      if ! manifest_for "$SRC" | diff -q - "$MANIFEST" >/dev/null; then
        echo "WARN: source repo protos drifted from vendored copy; re-run sync + differential suite" >&2
        exit 2
      fi
      echo "OK: no drift vs source repo"
    else
      echo "WARN: source repo not present; drift check skipped"
    fi
    ;;

  *)
    echo "usage: $0 [--check]" >&2
    exit 64
    ;;
esac
