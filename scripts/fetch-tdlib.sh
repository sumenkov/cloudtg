#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="${CLOUDTG_TDLIB_REPO:-}"

if [[ -z "${REPO}" ]]; then
  if git -C "$ROOT_DIR" remote get-url origin >/dev/null 2>&1; then
    remote="$(git -C "$ROOT_DIR" remote get-url origin)"
    if [[ "$remote" =~ github.com[:/](.+?)(\.git)?$ ]]; then
      REPO="${BASH_REMATCH[1]}"
    fi
  fi
fi

if [[ -z "${REPO}" ]]; then
  echo "Не удалось определить репозиторий GitHub. Укажи CLOUDTG_TDLIB_REPO=owner/repo."
  exit 1
fi

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Linux) os_id="linux" ;;
  Darwin) os_id="macos" ;;
  *) echo "Неподдерживаемая ОС: $os" ; exit 1 ;;
esac

case "$arch" in
  x86_64|amd64) arch_id="x86_64" ;;
  arm64|aarch64) arch_id="aarch64" ;;
  *) echo "Неподдерживаемая архитектура: $arch" ; exit 1 ;;
esac

platform="${os_id}-${arch_id}"
asset="tdlib-${platform}.zip"

token="${GITHUB_TOKEN:-${GH_TOKEN:-}}"
headers=(-H "Accept: application/vnd.github+json")
if [[ -n "${token}" ]]; then
  headers+=(-H "Authorization: Bearer ${token}")
fi

if ! command -v node >/dev/null 2>&1; then
  echo "Не найден node. Установи Node.js 18+ или укажи ссылку на артефакт вручную."
  exit 1
fi

json="$(curl -fsSL "${headers[@]}" "https://api.github.com/repos/${REPO}/releases/latest")"
url="$(node -e '
const fs = require("fs");
const data = JSON.parse(fs.readFileSync(0, "utf8"));
const name = process.argv[1];
const asset = (data.assets || []).find((a) => a.name === name);
if (!asset) process.exit(2);
process.stdout.write(asset.browser_download_url);
' "$asset" <<<"$json")" || {
  echo "Не найден артефакт ${asset} в релизе."
  exit 1
}

dest="${ROOT_DIR}/src-tauri/resources/tdlib/${platform}"
mkdir -p "$dest"
tmp="$(mktemp -t tdlib.XXXXXX.zip)"

curl -fL -o "$tmp" "$url"

if command -v unzip >/dev/null 2>&1; then
  unzip -o "$tmp" -d "$dest" >/dev/null
else
  if ! command -v python3 >/dev/null 2>&1; then
    echo "Не найден unzip или python3 для распаковки."
    exit 1
  fi
  python3 - <<'PY' "$tmp" "$dest"
import sys, zipfile
zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])
PY
fi

rm -f "$tmp"
echo "TDLib скачан в ${dest}"
