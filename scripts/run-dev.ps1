Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Push-Location (Split-Path $PSScriptRoot -Parent)

Write-Host "== CloudTG: install =="
npm install

Write-Host "== CloudTG: tauri dev =="
npm run tauri:dev

Pop-Location
