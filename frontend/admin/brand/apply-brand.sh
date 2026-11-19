#!/usr/bin/env bash
# apply-brand.sh — 在 sync-frontend.sh 同步完上游后，为快照打 RushWind 品牌覆写：
#   1. overlay/<frontend>/ 树原样覆盖到快照（logo / favicon / 登录页品牌插画）
#   2. 对声明文件做品牌文案替换（GoWind / 风行 → RushWind / 锐风）
# 品牌覆写是快照**唯一被允许的**对上游偏离：全部改动必须收在本目录内，
# 新增偏离时同步更新 overlay/<frontend>/ 与下方替换表，并在 brand/README.md 登记。
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

case "$FRONTEND" in
  react)
    # 上游 preferences 偏好默认值与 _core/auth.json 文案已随结构重组迁走
    # （app.name 走 VITE_APP_TITLE，登录/版权文案进组件层），可替换面收窄
    # 到 index.html 与 .env；CSS 变量名/存储命名空间（gowind-*）是功能标识，
    # 与 GOWIND_PASSWORD_RSA_PRIVATE_KEY 同性质，不换。
    replace index.html \
      -e 's/content="GoWind Admin React AntD Vite"/content="RushWind Admin React AntD Vite"/' \
      -e 's/name="author" content="GoWind"/name="author" content="RushWind"/'

    replace .env \
      -e 's/VITE_APP_TITLE="GoWind Admin"/VITE_APP_TITLE="RushWind Admin"/' \
      -e 's/VITE_APP_NAMESPACE="gowind-admin"/VITE_APP_NAMESPACE="rushwind-admin"/'
    ;;

  vue-element)
    # 上游 preferences default.ts 与 update-css-variables.ts 已迁走，落点收窄。
    replace src/bootstrap.ts \
      -e 's/|| "GoWind Admin"/|| "RushWind Admin"/'

    replace src/constants/index.ts \
      -e 's/APP_PREFIX = "gowind"/APP_PREFIX = "rushwind"/'

    replace vite.config.ts \
      -e 's/gowind-csp-meta/rushwind-csp-meta/'

    replace src/locales/zh-CN/core.json \
      -e 's/"headerTitle": "GoWind Admin"/"headerTitle": "RushWind Admin"/' \
      -e 's/风行中后台管理系统/锐风中后台管理系统/' \
      -e 's/Copyright © 2021 - 2026 GoWind/Copyright © 2021 - 2026 RushWind/' \
      -e 's/Copyright © 2026 GoWind/Copyright © 2026 RushWind/'

    replace src/locales/en-US/core.json \
      -e 's/"headerTitle": "GoWind Admin"/"headerTitle": "RushWind Admin"/' \
      -e 's/"brandTitle": "GoWind Admin System"/"brandTitle": "RushWind Admin System"/' \
      -e 's/Copyright © 2021 - 2026 GoWind/Copyright © 2021 - 2026 RushWind/' \
      -e 's/Copyright © 2026 GoWind/Copyright © 2026 RushWind/'

    replace .env \
      -e 's/VITE_APP_TITLE="GoWind Admin"/VITE_APP_TITLE="RushWind Admin"/' \
      -e 's/VITE_APP_NAMESPACE=gowind-admin/VITE_APP_NAMESPACE=rushwind-admin/'
    ;;

  vue-vben)
    # vben5 单包结构（上游自 monorepo 重构）：登录插画与版权组件在
    # framework/effects/layouts，license 横幅插件在 build-config/vite-config，
    # 应用面在 src/，env 在仓库根。上游 preferences 配置与 framework/locales
    # 的登录文案已迁走（风行字样上游已清零），落点收窄。
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

echo "brand overlay applied ($FRONTEND) -> $DST"
