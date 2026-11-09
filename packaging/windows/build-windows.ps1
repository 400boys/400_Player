#Requires -Version 5.1
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $Root

Write-Host "==> Compilation de Lumen400 v0.1.0 (x86_64-pc-windows-msvc, 100% Pur Rust)..." -ForegroundColor Magenta
cargo build --release -p lumen400

$Dist = Join-Path $Root "dist-windows"
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
Copy-Item "target\release\lumen400.exe" (Join-Path $Dist "lumen400.exe") -Force
Copy-Item "README.md" (Join-Path $Dist "README.md") -Force
Copy-Item "README.en.md" (Join-Path $Dist "README.en.md") -Force
Copy-Item "LICENSE-MIT" (Join-Path $Dist "LICENSE-MIT") -Force
Copy-Item "LICENSE-APACHE" (Join-Path $Dist "LICENSE-APACHE") -Force
Copy-Item "NOTICE" (Join-Path $Dist "NOTICE") -Force

$ZipPath = Join-Path $Root "lumen400-0.1.0-x86_64-pc-windows-msvc.zip"
if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force }
Compress-Archive -Path "$Dist\*" -DestinationPath $ZipPath

Write-Host "==> Binaire autonome : $Dist\lumen400.exe" -ForegroundColor Green
Write-Host "==> Archive portable : $ZipPath" -ForegroundColor Green
