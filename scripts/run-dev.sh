#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== CloudTG: install =="
npm install

echo "== CloudTG: tauri dev =="
npm run tauri:dev
