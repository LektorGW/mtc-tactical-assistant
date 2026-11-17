$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location (Join-Path $projectRoot 'apps/desktop')
try {
    npm.cmd run tauri build
    if ($LASTEXITCODE -ne 0) { throw 'Build Tauri nie przeszedł' }
    $releaseVersion = (Get-Content -Raw -Encoding UTF8 package.json | ConvertFrom-Json).version
    $releaseDir = Join-Path $projectRoot 'release'
    New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'target/release/mtc-tactical-assistant.exe') -Destination (Join-Path $releaseDir "MTC-Tactical-Assistant-$releaseVersion.exe")
    Get-ChildItem -LiteralPath (Join-Path $projectRoot 'target/release/bundle/nsis') -Filter "*_$($releaseVersion)_x64-setup.exe" | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $releaseDir }
    Copy-Item -LiteralPath (Join-Path $projectRoot 'README.md') -Destination $releaseDir
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/limitations-and-compliance.md') -Destination $releaseDir
    $licenseDir = Join-Path $releaseDir 'licenses'
    New-Item -ItemType Directory -Force -Path $licenseDir | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'data/weapons/community/LICENSE.md') -Destination (Join-Path $licenseDir 'mtc-community-MIT.txt')
    Copy-Item -LiteralPath (Join-Path $projectRoot 'data/weapons/community/manifest.json') -Destination (Join-Path $licenseDir 'community-manifest.json')
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/community-sources.md') -Destination $releaseDir
    Get-ChildItem -LiteralPath $releaseDir -Filter '*.exe' | Get-FileHash -Algorithm SHA256 | ForEach-Object { "$($_.Hash)  $([IO.Path]::GetFileName($_.Path))" } | Set-Content -Encoding UTF8 (Join-Path $releaseDir 'SHA256SUMS.txt')
} finally { Pop-Location }

