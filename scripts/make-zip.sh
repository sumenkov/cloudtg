#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
ZIP_NAME="cloudtg.zip"
rm -f "../$ZIP_NAME"
# exclude runtime folders and dependencies
zip -r "../$ZIP_NAME" . -x "node_modules/*" -x "ui/node_modules/*" -x "src-tauri/target/*" -x "ui/dist/*" -x "data/*" -x "cache/*" -x "logs/*"
echo "Created ../$ZIP_NAME"
