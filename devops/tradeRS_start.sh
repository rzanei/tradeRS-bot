#!/bin/bash
set -euo pipefail

echo "🔄 Starting TradeRS Bot setup..."

# Env variables must already be set in container
: "${GITHUB_TOKEN:?GITHUB_TOKEN not set}"
: "${REPO:=rzanei/tradeRS-bot}"
: "${VERSION:=latest}"
: "${ARCH:=linux-amd64}"
: "${BINARY_NAME:=tradeRS-bot}"
DEST_NAME="/usr/local/bin/${BINARY_NAME}"

if [[ "$VERSION" == "latest" ]]; then
  VERSION=$(curl -s -H "Authorization: token ${GITHUB_TOKEN}" \
    "https://api.github.com/repos/${REPO}/releases/latest" \
    | jq -r .tag_name)
fi

ASSET_NAME="${BINARY_NAME}-${VERSION}-${ARCH}"

echo "🔍 Downloading $ASSET_NAME from $REPO..."

RELEASE_DATA=$(curl -s -H "Authorization: token ${GITHUB_TOKEN}" \
  "https://api.github.com/repos/${REPO}/releases" \
  | jq ".[] | select(.tag_name == \"${VERSION}\")")

ASSET_ID=$(echo "$RELEASE_DATA" | jq ".assets[] | select(.name == \"$ASSET_NAME\") | .id")

if [[ -z "$ASSET_ID" || "$ASSET_ID" == "null" ]]; then
  echo "❌ Asset not found: $ASSET_NAME"
  exit 1
fi

curl -L \
  -H "Authorization: token ${GITHUB_TOKEN}" \
  -H "Accept: application/octet-stream" \
  "https://api.github.com/repos/${REPO}/releases/assets/${ASSET_ID}" \
  -o "$DEST_NAME"

chmod +x "$DEST_NAME"

echo "✅ Bot installed. Executing..."
exec "$DEST_NAME"
