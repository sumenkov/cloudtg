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
$manifestName = "tdlib-manifest.json"

$headers = @{ "Accept" = "application/vnd.github+json" }
if ($env:GITHUB_TOKEN) {
  $headers["Authorization"] = "Bearer $env:GITHUB_TOKEN"
} elseif ($env:GH_TOKEN) {
  $headers["Authorization"] = "Bearer $env:GH_TOKEN"
}

$manifestUrl = $env:CLOUDTG_TDLIB_MANIFEST_URL
if (-not $manifestUrl) {
  $api = "https://api.github.com/repos/$Repo/releases/latest"
  $release = Invoke-RestMethod -Uri $api -Headers $headers
  $manifestAsset = $release.assets | Where-Object { $_.name -eq $manifestName } | Select-Object -First 1
  if (-not $manifestAsset) {
    Write-Error "Не найден манифест $manifestName в релизе."
    exit 1
  }
  $manifestUrl = $manifestAsset.browser_download_url
}

$dest = Join-Path $RootDir "src-tauri/resources/tdlib/$platform"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$tmp = New-TemporaryFile

$manifestJson = Invoke-RestMethod -Uri $manifestUrl -Headers $headers
$entry = $manifestJson.assets | Where-Object { $_.platform -eq $platform } | Select-Object -First 1
if (-not $entry) {
  Write-Error "Не найден артефакт для платформы $platform в манифесте."
  exit 1
}

Invoke-WebRequest -Uri $entry.url -Headers $headers -OutFile $tmp
if ($entry.sha256) {
  $hash = (Get-FileHash -Algorithm SHA256 -Path $tmp).Hash.ToLower()
  if ($hash -ne $entry.sha256.ToLower()) {
    Write-Error "Checksum не совпадает."
    exit 1
  }
}

if ($entry.file -like "*.zip") {
  Expand-Archive -Path $tmp -DestinationPath $dest -Force
} elseif ($entry.file -like "*.tar.gz" -or $entry.file -like "*.tgz") {
  tar -xzf $tmp -C $dest
} else {
  Write-Error "Неизвестный формат архива: $($entry.file)"
  exit 1
}

Remove-Item $tmp -Force
Write-Host "TDLib скачан в $dest"
