# CoupCat packaging — builds the release exe and assembles dist\CoupCat\ with
# every runtime dependency (FFmpeg DLLs, ONNX Runtime, WebView2Loader, the
# Whisper caption model) so the folder runs as-is on another Windows machine.
# Run from anywhere:  powershell -ExecutionPolicy Bypass -File scripts\package.ps1
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$src  = Join-Path $root "src"
$tp   = Join-Path $root "third_party"

# MSVC developer environment (needed to build; see src\.cargo\config.toml).
$vc = "C:\Program Files\Microsoft Visual Studio\2022\Community"
Import-Module (Join-Path $vc "Common7\Tools\Microsoft.VisualStudio.DevShell.dll")
Enter-VsDevShell -VsInstallPath $vc -SkipAutomaticLocation -DevCmdArguments "-arch=x64 -host_arch=x64" | Out-Null

Set-Location $src
Write-Output "Building release…"
cargo build -p coupcat-desktop --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$dist = Join-Path $root "dist\CoupCat"
if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
New-Item -ItemType Directory -Force $dist | Out-Null

$rel = Join-Path $src "target\release"
Copy-Item (Join-Path $rel "coupcat.exe") $dist

# FFmpeg shared DLLs.
Copy-Item (Join-Path $tp "ffmpeg\ffmpeg-master-latest-win64-gpl-shared\bin\*.dll") $dist
# ONNX Runtime (background removal).
Copy-Item (Join-Path $tp "onnxruntime\onnxruntime-win-x64-1.32.0\lib\onnxruntime.dll") $dist
# Whisper caption model.
Copy-Item (Join-Path $tp "whisper\ggml-base.bin") $dist
# WebView2Loader, from the webview2-com-sys build output.
$wv = Get-ChildItem (Join-Path $rel "build") -Recurse -Filter WebView2Loader.dll -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match "x64" } | Select-Object -First 1
if ($wv) { Copy-Item $wv.FullName $dist }
# Licenses (AGPL-3.0).
foreach ($f in @("LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.md")) {
    $p = Join-Path $root $f
    if (Test-Path $p) { Copy-Item $p $dist }
}

@"
CoupCat — editor de vídeo libre (AGPL-3.0).
Ejecuta coupcat.exe. Todo lo necesario está en esta carpeta.
Requiere Windows 10/11 x64 con WebView2 Runtime (incluido en Windows 11).
"@ | Out-File -Encoding utf8 (Join-Path $dist "LEEME.txt")

$size = (Get-ChildItem $dist | Measure-Object -Property Length -Sum).Sum / 1MB
Write-Output ""
Write-Output ("Packaged to {0}  ({1:N0} MB)" -f $dist, $size)
Get-ChildItem $dist | Select-Object Name, @{N="MB";E={[math]::Round($_.Length/1MB,1)}}
