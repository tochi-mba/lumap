# Run by the installer before it replaces files: closes the lumap that is installed in the folder
# being upgraded, and only that one. A development build running from a checkout, or a copy of
# the portable zip elsewhere, is never touched.
[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$InstallDirectory)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

try {
    $installRoot = [IO.Path]::GetFullPath($InstallDirectory).TrimEnd('\')
    if (-not [IO.Directory]::Exists($installRoot)) { exit 0 }
    $prefix = $installRoot + '\'

    function Get-InstalledProcesses {
        Get-Process -Name lumap -ErrorAction SilentlyContinue | Where-Object {
            $exe = $_.Path
            $exe -and $exe.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)
        }
    }

    foreach ($process in @(Get-InstalledProcesses)) {
        try {
            if (-not $process.HasExited) { $process.CloseMainWindow() | Out-Null }
            if (-not $process.WaitForExit(5000)) { $process.Kill() }
            if (-not $process.WaitForExit(10000)) { throw 'Process did not exit.' }
        }
        catch {
            if (-not $process.HasExited) { throw }
        }
        finally { $process.Dispose() }
    }

    if (@(Get-InstalledProcesses).Count -gt 0) { throw 'An installed lumap restarted during shutdown.' }
    exit 0
}
catch {
    Write-Error -ErrorAction Continue ("Could not prepare lumap for upgrade: " + $_.Exception.Message)
    exit 1
}
