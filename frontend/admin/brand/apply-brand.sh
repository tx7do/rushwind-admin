#!/usr/bin/env bash
# apply-brand.sh — 在 sync-frontend.sh 同步完上游后，为快照打 RushWind 品牌覆写：
#   1. overlay/<frontend>/ 树原样覆盖到快照（logo / favicon / 登录页品牌插画 /
#      机制包品牌补丁脚本 / vben 应用侧文案覆写）
#   2. 对声明文件做品牌文案替换（GoWind / 风行 → RushWind / 锐风）
#   3. react / vue-element 额外把 brand-packages.mjs 挂到 package.json 的
#      postinstall（上游 2026-11-15 抽包后，preferences 默认值与 core 文案进了
#      机制包 dist，node_modules 不入库，须安装后由补丁脚本重写）
# 品牌覆写是快照**唯一被允许的**对上游偏离：全部改动必须收在本目录内，
# 新增偏离时同步更新 overlay/<frontend>/ 与下方替换表，并在 brand/README.md 登记。
#
# 2026-11-15 上游三端抽包（@go-wind/admin-react / admin-core+element / admin-vben）
# 后的目标文件大迁移：preferences 默认值与 core/auth 文案命名空间进包（react 的
# default.ts 与 _core/auth.json、element 的 default.ts 与 core.json、vben 的
# framework/core/preferences/config.ts 与 framework/locales 均 deleted），旧替换表
# 相应拆为三类：仍在宿主的走 sed；进包的走 postinstall 补丁（react/element）或
# 应用侧覆写文件（vben 源码形态包，defineOverridesPreferences + i18n 深合并天然
# 支持宿主覆盖）；__gowind-styles__ 注入标记与 CSP meta 插件分别进 element 包 /
# 回到 element 宿主 vite.config.ts。
set -euo pipefail

FRONTEND="${1:?usage: apply-brand.sh <react|vue-element|vue-vben> <snapshot-dir>}"
DST="${2:?usage: apply-brand.sh <react|vue-element|vue-vben> <snapshot-dir>}"
HERE="$(cd "$(dirname "$0")" && pwd)"
OVERLAY="$HERE/overlay/$FRONTEND"

# 1) overlay 覆盖
if [[ ! -d "$OVERLAY" ]]; then
  echo "ERROR: no brand overlay for frontend '$FRONTEND': $OVERLAY" >&2
  exit 1
fi
(cd "$OVERLAY" && find . -type f -print0) | while IFS= read -r -d '' f; do
  rel="${f#./}"
  mkdir -p "$DST/$(dirname "$rel")"
  cp "$OVERLAY/$rel" "$DST/$rel"
done

# 2) 品牌文案替换（逐条显式声明，不做全局盲替换）
replace() {
  local file="$1"
  shift
  sed -i "$@" "$DST/$file"
}

# 把 brand-packages.mjs 的 postinstall 钩子注入 package.json（幂等：已注入跳过）。
# 脚本本体由 overlay 拷入快照 patches/；钩子在每次 install 后驱动补丁。
hook_postinstall() {
  local pkg_json="$DST/package.json"
  if grep -q '"postinstall"' "$pkg_json"; then
    return 0
  fi
  if ! grep -q '"scripts": {' "$pkg_json"; then
    echo "ERROR: scripts block not found in $pkg_json; postinstall hook not injected" >&2
    exit 1
  fi
  sed -i 's|"scripts": {|"scripts": {\n    "postinstall": "node patches/brand-packages.mjs",|' "$pkg_json"
}

case "$FRONTEND" in
  react)
    replace index.html \
      -e 's/content="GoWind Admin React AntD Vite"/content="RushWind Admin React AntD Vite"/' \
      -e 's/name="author" content="GoWind"/name="author" content="RushWind"/'

    replace .env \
      -e 's/VITE_APP_TITLE="GoWind Admin"/VITE_APP_TITLE="RushWind Admin"/' \
      -e 's/VITE_APP_NAMESPACE="gowind-admin"/VITE_APP_NAMESPACE="rushwind-admin"/'

    # 机制包（@go-wind/admin-react）dist 内的品牌默认值与 auth 文案：postinstall 补丁。
    hook_postinstall
    ;;

  vue-element)
    replace src/bootstrap.ts \
      -e 's/|| "GoWind Admin"/|| "RushWind Admin"/'

    replace src/constants/index.ts \
      -e 's/APP_PREFIX = "gowind"/APP_PREFIX = "rushwind"/'

    replace vite.config.ts \
      -e 's/gowind-csp-meta/rushwind-csp-meta/'

    replace .env \
      -e 's/VITE_APP_TITLE="GoWind Admin"/VITE_APP_TITLE="RushWind Admin"/' \
      -e 's/VITE_APP_NAMESPACE=gowind-admin/VITE_APP_NAMESPACE=rushwind-admin/'

    # 机制包（@go-wind/admin-element）dist 内的品牌默认值与 core 文案 + 样式注入
    # 标记：postinstall 补丁（admin-core 实测零品牌串，不补）。
    hook_postinstall
    ;;

  vue-vben)
    # vben5 分层形态（2026-11-15 抽包）：冻结层进 @go-wind/admin-vben（npm 源码
    # 形态包），应用侧仍留 defineOverridesPreferences 与宿主 locales 两类官方
    # 覆写通道，品牌默认值与登录页文案走这两条；版权组件与 license 横幅留在
    # 宿主 framework/effects 与 build-config，继续走 sed。
    if ! grep -q 'copyright:' "$DST/src/preferences.ts"; then
      replace src/preferences.ts \
        -e "s|^});|  copyright: {\n    companyName: 'RushWind',\n    companySiteLink: 'https://github.com/tx7do/rushwind',\n  },\n});|"
    fi

    replace framework/effects/layouts/basic/copyright/copyright.vue \
      -e "s/companyName: 'GoWind Admin'/companyName: 'RushWind Admin'/"

    replace build-config/vite-config/src/plugins/license.ts \
      -e 's/ \* GoWind Admin/ * RushWind Admin/' \
      -e 's/Copyright (C) 2026 GoWind/Copyright (C) 2026 RushWind/'

    replace .env \
      -e 's/VITE_APP_TITLE="GoWind Admin"/VITE_APP_TITLE="RushWind Admin"/' \
      -e 's/VITE_APP_NAMESPACE=gowind-admin/VITE_APP_NAMESPACE=rushwind-admin/'
    ;;
esac

# 登录页 zh pageTitle（风行→锐风）：包 core 文案，宿主 locales 深合并覆盖
# （overlay 拷入 src/locales/langs/zh-CN/authentication.json，键级覆写）。
# react 的登录页文案在包 auth 命名空间内，由 postinstall 补丁覆盖；
# element 的在包 core 命名空间内，同上。

echo "brand overlay applied ($FRONTEND) -> $DST"
