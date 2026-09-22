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
    replace src/core/preferences/config/default.ts \
      -e 's/name: "GoWind Admin"/name: "RushWind Admin"/' \
      -e 's/companyName: "GoWind"/companyName: "RushWind"/' \
      -e 's|companySiteLink: "https://www.gowind.cloud"|companySiteLink: "https://github.com/tx7do/rushwind"|'

    replace src/locales/zh-CN/_core/auth.json \
      -e 's/风行中后台管理系统/锐风中后台管理系统/' \
      -e 's/Copyright © {year} GoWind/Copyright © {year} RushWind/'

    replace src/locales/en-US/_core/auth.json \
      -e 's/GoWind Admin Management System/RushWind Admin Management System/' \
      -e 's/Copyright © {year} GoWind/Copyright © {year} RushWind/'

    replace index.html \
      -e 's/content="GoWind Admin React AntD Vite"/content="RushWind Admin React AntD Vite"/' \
      -e 's/name="author" content="GoWind"/name="author" content="RushWind"/'

    replace .env \
      -e 's/VITE_APP_TITLE="GoWind Admin"/VITE_APP_TITLE="RushWind Admin"/' \
      -e 's/VITE_APP_NAMESPACE="gowind-admin"/VITE_APP_NAMESPACE="rushwind-admin"/'
    ;;

  vue-element)
    replace src/core/preferences/config/default.ts \
      -e 's/name: "GoWind Admin"/name: "RushWind Admin"/' \
      -e 's/companyName: "GoWind"/companyName: "RushWind"/' \
      -e 's|companySiteLink: "https://www.gowind.cloud"|companySiteLink: "https://github.com/tx7do/rushwind"|'

    replace src/bootstrap.ts \
      -e 's/|| "GoWind Admin"/|| "RushWind Admin"/'

    replace src/constants/index.ts \
      -e 's/APP_PREFIX = "gowind"/APP_PREFIX = "rushwind"/'

    replace src/core/preferences/update-css-variables.ts \
      -e 's/__gowind-styles__/__rushwind-styles__/'

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
    # vben5 monorepo：偏好默认值在 packages/@core，登录插画与版权组件在
    # packages/effects/layouts，多语言在 packages/locales，应用面在 apps/admin。
    # 测试快照（config.test.ts.snap）随默认值同步替换，保持上游测试语义。
    replace packages/@core/preferences/src/config.ts \
      -e "s/companyName: 'GoWind'/companyName: 'RushWind'/" \
      -e "s|companySiteLink: 'https://www.gowind.cloud'|companySiteLink: 'https://github.com/tx7do/rushwind'|"

    replace packages/@core/preferences/__tests__/__snapshots__/config.test.ts.snap \
      -e 's/"companyName": "GoWind"/"companyName": "RushWind"/' \
      -e 's|"companySiteLink": "https://www.gowind.cloud"|"companySiteLink": "https://github.com/tx7do/rushwind"|'

    replace packages/effects/layouts/src/basic/copyright/copyright.vue \
      -e "s/companyName: 'GoWind Admin'/companyName: 'RushWind Admin'/"

    replace packages/locales/src/langs/zh-CN/authentication.json \
      -e 's/风行中后台管理系统/锐风中后台管理系统/'

    replace internal/vite-config/src/plugins/license.ts \
      -e 's/ \* GoWind Admin/ * RushWind Admin/' \
      -e 's/Copyright (C) 2026 GoWind/Copyright (C) 2026 RushWind/'

    replace apps/admin/.env \
      -e 's/VITE_APP_TITLE="GoWind Admin"/VITE_APP_TITLE="RushWind Admin"/' \
      -e 's/VITE_APP_NAMESPACE=gowind-admin/VITE_APP_NAMESPACE=rushwind-admin/'
    ;;
esac

echo "brand overlay applied ($FRONTEND) -> $DST"
