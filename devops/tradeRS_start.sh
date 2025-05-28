#!/bin/bash
set -euo pipefail

TOKEN="${GITHUB_TOKEN:?Missing GITHUB_TOKEN}"
REPO="rzanei/tradeRS-bot"
VERSION="v0.0.5-rc1"
ASSET_NAME="tradeRS-bot-${VERSION}-linux-amd64"
DEST_NAME="tradeRS-bot"

echo "🔎 Getting asset ID from GitHub API..."

RELEASE_DATA=$(curl -s -H "Authorization: token ${TOKEN}" \
  "https://api.github.com/repos/${REPO}/releases/tags/${VERSION}")

ASSET_ID=$(echo "$RELEASE_DATA" | jq ".assets[] | select(.name == \"$ASSET_NAME\") | .id")

if [[ -z "$ASSET_ID" || "$ASSET_ID" == "null" ]]; then
  echo "❌ Asset '${ASSET_NAME}' not found or access denied"
  echo "$RELEASE_DATA" | jq '.'
  exit 1
fi

echo "⬇️ Downloading asset ID $ASSET_ID as $DEST_NAME..."

curl -L \
  -H "Authorization: token ${TOKEN}" \
  -H "Accept: application/octet-stream" \
  "https://api.github.com/repos/${REPO}/releases/assets/${ASSET_ID}" \
  -o "$DEST_NAME"

chmod +x "$DEST_NAME"
echo "✅ Done: $DEST_NAME is ready"
exec /usr/local/bin/${BINARY_NAME}
