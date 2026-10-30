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

# Verify Windows recognises the EXE's embedded icon (not the generic application icon).
$exe = Join-Path $app "lumap.exe"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class LumapIconResource {
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    public static extern uint ExtractIconEx(string file, int index, IntPtr large, IntPtr small, uint count);
}
"@
if ([LumapIconResource]::ExtractIconEx($exe, -1, [IntPtr]::Zero, [IntPtr]::Zero, 0) -lt 1) {
    throw "lumap.exe has no embedded Windows icon resource."
}

# Start-menu actions must point at the installed executable with the correct launch flags.
$programs = [Environment]::GetFolderPath("Programs")
$shell = New-Object -ComObject WScript.Shell
$links = @(
    @{ File = "lumap.lnk"; Args = "" },
    @{ File = "lumap - New show.lnk"; Args = "--new-show" },
    @{ File = "lumap - Open show.lnk"; Args = "--open-show" },
    @{ File = "lumap - Check for updates.lnk"; Args = "--check-updates" }
)
foreach ($item in $links) {
    $link = Join-Path $programs $item.File
    if (-not (Test-Path $link)) { throw "Missing Start menu shortcut: $link" }
    $shortcut = $shell.CreateShortcut($link)
    if ($shortcut.Arguments -ne $item.Args) {
        throw "Shortcut $link arguments were '$($shortcut.Arguments)' instead of '$($item.Args)'."
    }
    if ($shortcut.TargetPath -ne $exe) {
        throw "Shortcut $link does not launch the installed app."
    }
}
Write-Host "Embedded app icon and Start menu shortcuts passed."

# The lumap command on the user's PATH (the registry value, since this shell predates it).
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$app*") { throw "The install folder was not added to the user PATH: $userPath" }

# A clean PATH: whatever the runner has must not be what makes video work.
$env:PATH = "$env:SystemRoot\system32;$env:SystemRoot"
$env:LUMAP_FFMPEG = $null
$exe = Join-Path $app "lumap.exe"
$demo = Join-Path $app "examples\two-projector-demo.lumap.toml"
$validate = Start-Process -FilePath $exe -ArgumentList "validate `"$demo`"" -Wait -PassThru
if ($validate.ExitCode -ne 0) { throw "The installed lumap cannot validate the demo project (exit $($validate.ExitCode))." }

$frames = Join-Path $env:TEMP "lumap-smoke-frames"
if (Test-Path $frames) { Remove-Item -Recurse -Force $frames }
$render = Start-Process -FilePath $exe -ArgumentList "render `"$demo`" --out `"$frames`" --size 320x180" -Wait -PassThru
if ($render.ExitCode -ne 0) { throw "The installed lumap cannot render the demo project (exit $($render.ExitCode))." }
$pngs = @(Get-ChildItem $frames -Filter *.png)
if ($pngs.Count -lt 1) { throw "lumap render produced no PNG." }
Write-Host "Rendered $($pngs.Count) output(s) from the installed app."

# A pipeline ending in Select-Object -First can leave LASTEXITCODE unset and terminate
# the process early. Wait on the native process and inspect its exit code instead.
$ffmpegVersionFile = Join-Path $env:TEMP "lumap-smoke-ffmpeg-version.txt"
try {
    $ffmpeg = Start-Process -FilePath (Join-Path $app "ffmpeg\ffmpeg.exe") -ArgumentList "-version" -Wait -PassThru -RedirectStandardOutput $ffmpegVersionFile
    if ($ffmpeg.ExitCode -ne 0) { throw "The installed ffmpeg exited with $($ffmpeg.ExitCode)." }
    $versionLine = Get-Content $ffmpegVersionFile -TotalCount 1
    if (-not $versionLine -or $versionLine -notmatch '^ffmpeg version ') { throw "The installed ffmpeg did not return its version." }
    Write-Host "Installed $versionLine"
} finally {
    Remove-Item $ffmpegVersionFile -ErrorAction SilentlyContinue
}

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
