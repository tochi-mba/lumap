# Fetches the ffmpeg the installer ships: one pinned BtbN LGPL shared build of the 8.1 branch,
# verified against a SHA-256 recorded here. Only ffmpeg.exe, ffprobe.exe, the DLLs they load and
# the licence text are kept; ffplay is not shipped. Nothing links against ffmpeg (lumap runs it
# as a separate process), so lumap stays MIT OR Apache-2.0 and the LGPL copy sits beside it with
# its licence and a note of where its source is.
#
# Moving to a newer build is a deliberate commit that changes $Url and $Sha256 together.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Destination,
    # A folder where the zip is kept between runs, so a rebuild does not download it again.
    [string]$Cache = (Join-Path ([IO.Path]::GetTempPath()) "lumap-ffmpeg")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Tag = "autobuild-2026-10-28-19-47"
$Build = "ffmpeg-n8.1.3-25-g611cec792a-win64-lgpl-shared-8.1"
$Url = "https://github.com/BtbN/FFmpeg-Builds/releases/download/$Tag/$Build.zip"
$Sha256 = "0448c3f3b9bdc724cc0189d5ce6e6bfa5b8937e7ec408e1e97fb08719bdb3b92"

New-Item -ItemType Directory -Force -Path $Cache | Out-Null
$zip = Join-Path $Cache "$Build.zip"

function Get-Hash([string]$Path) { (Get-FileHash -Algorithm SHA256 $Path).Hash.ToLowerInvariant() }

if (-not (Test-Path $zip) -or (Get-Hash $zip) -ne $Sha256) {
    Write-Host "Downloading $Url"
    $ProgressPreference = "SilentlyContinue"
    Invoke-WebRequest -Uri $Url -OutFile $zip -UseBasicParsing
}
$actual = Get-Hash $zip
if ($actual -ne $Sha256) {
    Remove-Item -Force $zip
    throw "ffmpeg download does not match the pinned SHA-256 (expected $Sha256, got $actual). Not shipping it."
}

$extracted = Join-Path $Cache "extracted"
if (Test-Path $extracted) { Remove-Item -Recurse -Force $extracted }
Expand-Archive -Path $zip -DestinationPath $extracted
$bin = Join-Path $extracted "$Build\bin"

if (Test-Path $Destination) { Remove-Item -Recurse -Force $Destination }
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
foreach ($name in @("ffmpeg.exe", "ffprobe.exe")) {
    $file = Join-Path $bin $name
    if (-not (Test-Path $file)) { throw "$name is missing from the ffmpeg build." }
    Copy-Item $file $Destination
}
Get-ChildItem (Join-Path $bin "*.dll") | Copy-Item -Destination $Destination
Copy-Item (Join-Path $extracted "$Build\LICENSE.txt") (Join-Path $Destination "LICENSE.txt")
@(
    "ffmpeg $Build",
    "Built by https://github.com/BtbN/FFmpeg-Builds (LGPL build), release $Tag.",
    "Downloaded from $Url",
    "SHA-256 of the archive: $Sha256",
    "Source: https://github.com/FFmpeg/FFmpeg and the build scripts in the repository above.",
    "lumap runs these programs as separate processes and does not link against them."
) | Set-Content -Path (Join-Path $Destination "VERSION.txt") -Encoding ascii

& (Join-Path $Destination "ffmpeg.exe") -version 2>&1 | Select-Object -First 1 | ForEach-Object { Write-Host "Bundled $_" }
if ($LASTEXITCODE -ne 0) { throw "The bundled ffmpeg does not run." }
