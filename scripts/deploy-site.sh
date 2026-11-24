#!/usr/bin/env bash
# 把 desktop/site/ 发布到自建服务器上的官网（1Panel + OpenResty）。
# 用法：./scripts/deploy-site.sh [--dry-run]
set -euo pipefail

KEY="${SSH_KEY:-$HOME/.ssh/1panel_114}"
HOST="${DEPLOY_HOST:-root@114.66.54.114}"
SITE="tinymusic.carljoy.cn"
DEST="/opt/1panel/www/sites/${SITE}/index/"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${ROOT}/desktop/site/"

if [[ ! -f "$SRC/index.html" && ! -f "$SRC/../index.html" ]]; then
  echo "找不到站点源目录：$SRC" >&2
  exit 1
fi

DRY=""
if [[ "${1:-}" == "--dry-run" ]]; then DRY="--dry-run"; fi

echo "==> $SRC  ->  $HOST:$DEST"
rsync -az --delete $DRY -e "ssh -i \"$KEY\" -o StrictHostKeyChecking=no" "$SRC" "$HOST:$DEST"

if [[ -z "$DRY" ]]; then
  ssh -i "$KEY" -o StrictHostKeyChecking=no "$HOST" \
    "chown -R root:root '$DEST' && find '$DEST' -type d -exec chmod 755 {} + && find '$DEST' -type f -exec chmod 644 {} +"
  echo "==> 已发布：https://${SITE}/"
fi
