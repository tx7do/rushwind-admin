#!/usr/bin/env bash
# sync-frontend.sh — 把上游前端同步进本仓 frontend/admin/<frontend>/，打 RushWind
# 品牌覆写，并维护双清单：
#
#   frontend/admin/sync-frontend.sh <frontend>          # 同步 + 品牌覆写
#                                                       #   + 重建 <frontend>.MANIFEST.sha256
#                                                       #   与 <frontend>.UPSTREAM.sha256
#   frontend/admin/sync-frontend.sh <frontend> --check  # CI 门：快照终态必须与
#                                                       #   <frontend>.MANIFEST.sha256 逐字节一致
#                                                       #   （防手改，退出码 1）；源仓在时另对比
#                                                       #   <frontend>.UPSTREAM.sha256 做漂移报告
#                                                       #   （退出码 2，CI 无源仓自跳过）
#
# <frontend> ∈ react | vue-element | vue-vben。同步源默认取
# /d/GoProject/go-wind-admin/frontend/admin/<frontend>，可用 <大写前端名>_FRONTEND_SRC
# 覆盖（REACT_FRONTEND_SRC / VUE_ELEMENT_FRONTEND_SRC）。
#
# 快照唯一被允许的对上游偏离是 RushWind 品牌覆写（brand/，同步后自动施加）。
# 除此之外任何手改都视为契约破坏；上游变更必须以整树重同步的方式显式接受
# （重建双清单并跑差分回归）。
# 字节保真：不归一化、不做 EOL 转换（.gitattributes 对各前端快照与双清单钉了 -text）。
# 清单排序钉死 C locale：UTF-8 环境的字典序会把 dotfile 排到字母之后，同一棵树在
# Windows（C locale）与 Linux（en_US.UTF-8）上会产出顺序不同的清单，纯属排序噪声
# 却足以挂掉 --check 的逐行 diff。
set -euo pipefail
export LC_ALL=C

FRONTEND="${1:-}"
MODE="${2:-sync}"
case "$FRONTEND" in
  react|vue-element|vue-vben) ;;
  *)
    echo "usage: $0 <react|vue-element|vue-vben> [--check]" >&2
    exit 64
    ;;
esac

HERE="$(cd "$(dirname "$0")" && pwd)"
DST="$HERE/$FRONTEND"
MANIFEST="$HERE/$FRONTEND.MANIFEST.sha256"
UPSTREAM_MANIFEST="$HERE/$FRONTEND.UPSTREAM.sha256"
SRC_VAR="$(printf '%s' "$FRONTEND" | tr 'a-z-' 'A-Z_')_FRONTEND_SRC"
SRC="${!SRC_VAR:-/d/GoProject/go-wind-admin/frontend/admin/$FRONTEND}"

# 源仓文件清单：优先 git ls-files（精确排除 node_modules/dist/.idea 等未跟踪内容），
# 非 git 目录退化为 find 剪枝。
list_src() {
  if git -C "$SRC" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git -C "$SRC" ls-files -z .
  else
    (cd "$SRC" && find . -type d \( -name node_modules -o -name dist -o -name dist.zip -o -name .idea -o -name .turbo \) -prune \
      -o -type f -print0 | sed -z 's|^\./||')
  fi
}

# 源仓原始树哈希清单（漂移基线）。
upstream_hash() {
  list_src | sort -z | while IFS= read -r -d '' f; do
    printf '%s  %s\n' "$(sha256sum "$SRC/$f" | cut -d' ' -f1)" "$f"
  done
}

# 对一个树生成排序 sha256 清单（路径 + 字节哈希，无任何归一化）。
#
# 哈希宇宙 = 树上现有文件 − git 忽略路径。忽略路径（.gitignore：node_modules/
# dist/.vite 等）永远不会出现在 CI 的 checkout 里——本地 dev server 会在快照内
# 落构建缓存（.vite/deps），且 sync 的目录清空在句柄被握住时静默失败，残留缓存
# 若被哈希进清单就是"本地过、CI 挂"。git 不可用时硬失败，宁可挂门也不放水。
# 剪枝 node_modules/dist：不入库的构建产物，git 亦忽略之，此处剪枝纯为省扫描。
hash_tree() {
  (
    cd "$1" || exit 1
    git rev-parse --is-inside-work-tree >/dev/null 2>&1 || {
      echo "ERROR: $1 is not a git work tree; cannot exclude ignored paths" >&2
      exit 1
    }
    candidates=()
    while IFS= read -r -d '' f; do candidates+=("$f"); done < <(
      find . -type d \( -name node_modules -o -name dist \) -prune \
        -o -type f -print0 | sed -z 's|^\./||' | sort -z
    )
    ((${#candidates[@]})) || exit 0
    declare -A ignored=()
    while IFS= read -r -d '' f; do ignored["$f"]=1; done < <(
      printf '%s\0' "${candidates[@]}" | git check-ignore --stdin -z 2>/dev/null || true
    )
    for f in "${candidates[@]}"; do
      [[ -n "${ignored[$f]:-}" ]] && continue
      [[ -f "$f" ]] || continue
      printf '%s  %s\n' "$(sha256sum "$f" | cut -d' ' -f1)" "$f"
    done
  )
}

case "$MODE" in
  sync)
    if [[ ! -d "$SRC" ]]; then
      echo "ERROR: $FRONTEND source not found: $SRC (set $SRC_VAR)" >&2
      exit 1
    fi
    # 只清被跟踪的内容：node_modules/ 与 dist/ 不入库，原地保留（重装依赖太贵，
    # 且 dev server 常以本目录为 cwd，顶层目录句柄被握住时删目录会直接失败）。
    find "$DST" -mindepth 1 -maxdepth 1 ! -name node_modules ! -name dist -exec rm -rf {} + 2>/dev/null || true
    mkdir -p "$DST"
    list_src | sort -z | while IFS= read -r -d '' f; do
      mkdir -p "$DST/$(dirname "$f")"
      cp "$SRC/$f" "$DST/$f"
    done
    # RushWind 品牌覆写（快照唯一允许的上游偏离）。
    bash "$HERE/brand/apply-brand.sh" "$FRONTEND" "$DST"
    upstream_hash > "$UPSTREAM_MANIFEST"
    hash_tree "$DST" > "$MANIFEST"
    echo "synced $(grep -c . "$UPSTREAM_MANIFEST") upstream files -> $DST (manifests: $MANIFEST, $UPSTREAM_MANIFEST)"
    ;;
  --check)
    status=0
    # 防手改门：快照终态必须与 MANIFEST 逐字节一致（多余/缺失/改动都算失败）。
    # 失败时打印差异前 40 行：与 sync-protos.sh 对齐，静默吞 diff 只会浪费排查。
    if d="$(diff -u "$MANIFEST" <(hash_tree "$DST") 2>&1)"; then
      echo "OK: frontend/admin/$FRONTEND/ matches $FRONTEND.MANIFEST.sha256"
    else
      echo "ERROR: frontend/admin/$FRONTEND/ does not match $FRONTEND.MANIFEST.sha256 (hand-edit? run sync-frontend.sh $FRONTEND to rebuild)" >&2
      printf '%s\n' "$d" | head -40 >&2 || true
      status=1
    fi
    # 漂移检测：源仓在时对比上游基线（CI 无源仓时自跳过）。
    if [[ -d "$SRC" ]] && git -C "$SRC" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
      if ! diff -u "$UPSTREAM_MANIFEST" <(upstream_hash) >/dev/null; then
        echo "WARNING: upstream $FRONTEND source has drifted from the baseline; re-run sync-frontend.sh $FRONTEND to accept" >&2
        [[ $status -eq 0 ]] && status=2
      fi
    else
      echo "WARNING: upstream $FRONTEND source not present; drift check skipped" >&2
    fi
    exit $status
    ;;
  *)
    echo "usage: $0 <react|vue-element|vue-vben> [--check]" >&2
    exit 64
    ;;
esac
