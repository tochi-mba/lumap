# Builds lumap for Windows and wraps it two ways: a per-user installer
# (dist\lumap-Setup-<version>.exe) and a portable zip (dist\lumap-<version>-windows-x64.zip),
# each with a .sha256 file beside it. Both carry the bundled ffmpeg, the demo project, the
# manual and the licences.
#
#   installer\build.ps1 -Version 0.3.0            build everything
#   installer\build.ps1 -Version 0.3.0 -SkipBuild  reuse target\release\lumap.exe
[CmdletBinding()]
param(
    [string]$Version = "0.0.0",
    [switch]$SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($Version -notmatch '^\d+\.\d+\.\d+(?:[-.][0-9A-Za-z.-]+)?$') { throw "'$Version' is not a version." }

$root = Resolve-Path (Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "..")
$dist = Join-Path $root "dist"
$payload = Join-Path $dist "lumap"
if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
New-Item -ItemType Directory -Path $payload | Out-Null

if (-not $SkipBuild) {
    Write-Host "Building lumap $Version (release, spout)"
    & cargo build --release -p lumap-app --features spout --locked
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed." }
}
$exe = Join-Path $root "target\release\lumap.exe"
if (-not (Test-Path $exe)) { throw "target\release\lumap.exe is missing." }
Copy-Item $exe $payload

& (Join-Path $root "installer\fetch-ffmpeg.ps1") -Destination (Join-Path $payload "ffmpeg")

New-Item -ItemType Directory -Path (Join-Path $payload "examples") | Out-Null
Copy-Item (Join-Path $root "examples\*.lumap.toml") (Join-Path $payload "examples")
New-Item -ItemType Directory -Path (Join-Path $payload "docs") | Out-Null
Copy-Item (Join-Path $root "docs\manual.md") (Join-Path $payload "docs")
foreach ($file in @("README.md", "CHANGELOG.md", "LICENSE-MIT", "LICENSE-APACHE")) {
    Copy-Item (Join-Path $root $file) $payload
}

# The app finds the bundled ffmpeg beside itself; prove it before packaging.
$env:LUMAP_FFMPEG = $null
& (Join-Path $payload "lumap.exe") validate (Join-Path $payload "examples\two-projector-demo.lumap.toml")
if ($LASTEXITCODE -ne 0) { throw "The staged lumap.exe cannot validate the demo project." }

$iscc = @(
    (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
    (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
    (Get-Command ISCC.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue)
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 is not installed; get it from https://jrsoftware.org/isdl.php (winget install JRSoftware.InnoSetup)." }

& $iscc "/DAppVersion=$Version" "/DSource=$dist" "/O$dist" /Q (Join-Path $root "installer\lumap.iss")
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed." }

$zip = Join-Path $dist "lumap-$Version-windows-x64.zip"
Compress-Archive -Path (Join-Path $payload "*") -DestinationPath $zip

foreach ($file in @((Join-Path $dist "lumap-Setup-$Version.exe"), $zip)) {
    $hash = (Get-FileHash -Algorithm SHA256 $file).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText("$file.sha256", "$hash  $(Split-Path -Leaf $file)`n", [Text.Encoding]::ASCII)
    $mb = [math]::Round((Get-Item $file).Length / 1MB)
    Write-Host "Built $(Split-Path -Leaf $file) ($mb MB, $hash)"
}
