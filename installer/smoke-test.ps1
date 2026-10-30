# Proves an installer does what the website promises: installs for the current user without a
# prompt, puts lumap.exe, ffmpeg and hand models where the app looks, validates and renders the
# demo project with no ffmpeg on the PATH, registers the lumap command, and uninstalls cleanly.
# CI runs it on every push (build-windows) and before every release.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Installer)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$app = Join-Path $env:LOCALAPPDATA "Programs\lumap"
if (Test-Path $app) { Remove-Item -Recurse -Force $app }

Write-Host "Installing $Installer"
$setup = Start-Process -FilePath $Installer -ArgumentList "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/LOG=$env:TEMP\lumap-setup.log" -Wait -PassThru
if ($setup.ExitCode -ne 0) {
    Get-Content "$env:TEMP\lumap-setup.log" -Tail 40
    throw "The installer exited with $($setup.ExitCode)."
}

foreach ($needed in @("lumap.exe", "ffmpeg\ffmpeg.exe", "ffmpeg\ffprobe.exe", "ffmpeg\LICENSE.txt", "ffmpeg\VERSION.txt", "models\palm_detection_mediapipe_2023feb.onnx", "models\handpose_estimation_mediapipe_2023feb.onnx", "models\NOTICE.txt", "examples\two-projector-demo.lumap.toml", "docs\manual.md", "unins000.exe")) {
    if (-not (Test-Path (Join-Path $app $needed))) { throw "$needed was not installed." }
}

# The lumap command on the user's PATH (the registry value, since this shell predates it).
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$app*") { throw "The install folder was not added to the user PATH: $userPath" }

# A clean PATH: whatever the runner has must not be what makes video work.
$env:PATH = "$env:SystemRoot\system32;$env:SystemRoot"
$env:LUMAP_FFMPEG = $null
$exe = Join-Path $app "lumap.exe"
& $exe validate (Join-Path $app "examples\two-projector-demo.lumap.toml")
if ($LASTEXITCODE -ne 0) { throw "The installed lumap cannot validate the demo project." }

$frames = Join-Path $env:TEMP "lumap-smoke-frames"
if (Test-Path $frames) { Remove-Item -Recurse -Force $frames }
& $exe render (Join-Path $app "examples\two-projector-demo.lumap.toml") --out $frames --size 320x180
if ($LASTEXITCODE -ne 0) { throw "The installed lumap cannot render the demo project." }
$pngs = @(Get-ChildItem $frames -Filter *.png)
if ($pngs.Count -lt 1) { throw "lumap render produced no PNG." }
Write-Host "Rendered $($pngs.Count) output(s) from the installed app."

& (Join-Path $app "ffmpeg\ffmpeg.exe") -version 2>&1 | Select-Object -First 1 | ForEach-Object { Write-Host "Installed $_" }
if ($LASTEXITCODE -ne 0) { throw "The installed ffmpeg does not run." }

Write-Host "Uninstalling"
$remove = Start-Process -FilePath (Join-Path $app "unins000.exe") -ArgumentList "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART" -Wait -PassThru
if ($remove.ExitCode -ne 0) { throw "The uninstaller exited with $($remove.ExitCode)." }
# The uninstaller removes itself a moment after it exits.
$deadline = (Get-Date).AddSeconds(30)
while ((Test-Path $app) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
if (Test-Path $app) { throw "The install folder is still there after uninstalling: $app" }
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -like "*$app*") { throw "The install folder is still on the user PATH after uninstalling." }
Write-Host "Install, run and uninstall all passed."
