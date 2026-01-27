Param(
  [string]$Repo = $env:CLOUDTG_TDLIB_REPO
)

$RootDir = Resolve-Path (Join-Path $PSScriptRoot "..")

if (-not $Repo) {
  try {
    $remote = git -C $RootDir remote get-url origin 2>$null
    if ($remote -match "github.com[:/](.+?)(\.git)?$") {
      $Repo = $Matches[1]
    }
  } catch {
  }
}

if (-not $Repo) {
  Write-Error "Не удалось определить репозиторий GitHub. Укажи CLOUDTG_TDLIB_REPO=owner/repo."
  exit 1
}

$platform = "windows-x86_64"
$assetName = "tdlib-$platform.zip"

$headers = @{ "Accept" = "application/vnd.github+json" }
if ($env:GITHUB_TOKEN) {
  $headers["Authorization"] = "Bearer $env:GITHUB_TOKEN"
} elseif ($env:GH_TOKEN) {
  $headers["Authorization"] = "Bearer $env:GH_TOKEN"
}

$api = "https://api.github.com/repos/$Repo/releases/latest"
$release = Invoke-RestMethod -Uri $api -Headers $headers
$asset = $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
if (-not $asset) {
  Write-Error "Не найден артефакт $assetName в релизе."
  exit 1
}

$dest = Join-Path $RootDir "src-tauri/resources/tdlib/$platform"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$tmp = New-TemporaryFile

Invoke-WebRequest -Uri $asset.browser_download_url -Headers $headers -OutFile $tmp
Expand-Archive -Path $tmp -DestinationPath $dest -Force
Remove-Item $tmp -Force

Write-Host "TDLib скачан в $dest"
