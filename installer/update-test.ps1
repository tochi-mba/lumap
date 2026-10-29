# Proves the update path the app uses: installs the built installer into a temporary folder,
# runs it again the way lumap installs an update of itself (/relaunch=1) and checks that lumap
# opened again afterwards, then uninstalls. For CI's throwaway machines: it opens lumap's
# window briefly and changes the user's "Open with" offers while it runs.
[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$Installer)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$work = Join-Path ([IO.Path]::GetTempPath()) ("lumap-update-test-" + [guid]::NewGuid().ToString('N'))
$installDir = Join-Path $work 'app'
$prefix = $installDir + '\'

# The reopened lumap keeps its settings in the work folder and skips the welcome screen.
$env:LUMAP_CONFIG_DIR = Join-Path $work 'config'
New-Item -ItemType Directory -Force -Path $env:LUMAP_CONFIG_DIR | Out-Null
Set-Content -Path (Join-Path $env:LUMAP_CONFIG_DIR 'settings.toml') -Value "skip_welcome = true`n" -Encoding ascii

function Get-Installed {
    @(Get-Process -Name lumap -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) })
}

function Invoke-Installer([string[]]$Arguments) {
    # Waiting on the installer alone: Start-Process -Wait would also wait for the lumap an update reopens.
    $process = Start-Process -FilePath $Installer -ArgumentList $Arguments -PassThru
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) { throw "The installer exited with $($process.ExitCode)." }
}

try {
    Invoke-Installer @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=`"$installDir`"")
    if (-not (Test-Path (Join-Path $installDir 'lumap.exe'))) { throw 'lumap.exe was not installed.' }
    Start-Sleep -Seconds 3
    if (@(Get-Installed).Count -gt 0) { throw 'A silent install opened lumap; only an update should.' }

    Invoke-Installer @('/SILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CLOSEAPPLICATIONS', '/relaunch=1', "/DIR=`"$installDir`"")
    $deadline = (Get-Date).AddSeconds(30)
    while (@(Get-Installed).Count -eq 0 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
    if (@(Get-Installed).Count -eq 0) { throw 'The update did not open lumap again.' }
    Write-Host 'The update installed and opened lumap again.'

    # An update must also be able to close the running copy it is replacing.
    Invoke-Installer @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CLOSEAPPLICATIONS', "/DIR=`"$installDir`"")
    Start-Sleep -Seconds 3
    if (@(Get-Installed).Count -gt 0) { throw 'A quiet update left lumap running.' }
    Write-Host 'A quiet update closed the running lumap and replaced it.'

    Start-Process -FilePath (Join-Path $installDir 'unins000.exe') -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait | Out-Null
    $deadline = (Get-Date).AddSeconds(30)
    while ((Test-Path (Join-Path $installDir 'lumap.exe')) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
    if (Test-Path (Join-Path $installDir 'lumap.exe')) { throw 'Uninstalling left lumap.exe behind.' }
    Write-Host 'Uninstalled cleanly.'
}
finally {
    Get-Installed | ForEach-Object { $_.Kill(); $_.WaitForExit(10000) | Out-Null }
    $uninstaller = Join-Path $installDir 'unins000.exe'
    if (Test-Path $uninstaller) {
        Start-Process -FilePath $uninstaller -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait | Out-Null
    }
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
