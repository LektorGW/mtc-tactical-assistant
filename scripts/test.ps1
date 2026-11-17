$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location $projectRoot
try {
    cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatowanie Rust nie przeszło' }
    cargo clippy --workspace --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy nie przeszedł' }
    cargo test --workspace
    if ($LASTEXITCODE -ne 0) { throw 'Testy Rust nie przeszły' }
    cargo build -p mtc-capture --example hud_fixture
    if ($LASTEXITCODE -ne 0) { throw 'Build okna testowego nie przeszedł' }
    Push-Location 'apps/desktop'
    try {
        npm.cmd run build
        if ($LASTEXITCODE -ne 0) { throw 'Build interfejsu nie przeszedł' }
        npm.cmd test
        if ($LASTEXITCODE -ne 0) { throw 'Testy interfejsu nie przeszły' }
        npm.cmd audit
        if ($LASTEXITCODE -ne 0) { throw 'Audyt zależności nie przeszedł' }
    } finally { Pop-Location }
} finally { Pop-Location }

