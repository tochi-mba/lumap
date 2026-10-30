# Fetches the hand-tracking models lumap ships: Google MediaPipe's palm detector and hand
# landmark model (Apache-2.0) in the ONNX conversions published by OpenCV Zoo, each pinned to one
# revision of its Hugging Face repository and verified against a SHA-256 recorded here. lumap runs
# them with tract (pure Rust, compiled into lumap.exe), so no native library ships with them;
# models\NOTICE.txt carries their origin and licence and is copied beside them.
#
#   installer\fetch-models.ps1 -Destination models         what the tests and a dev build use
#   installer\fetch-models.ps1 -Destination dist\lumap\models   what the installer packs
#
# Moving to newer models is a deliberate commit that changes a Revision and its Sha256 together,
# and the same lines in models\NOTICE.txt (the script refuses to run when they disagree).
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Destination,
    # A folder where the downloads are kept between runs, so a rebuild does not fetch them again.
    [string]$Cache = (Join-Path ([IO.Path]::GetTempPath()) "lumap-models")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Models = @(
    @{
        Name     = "palm_detection_mediapipe_2023feb.onnx"
        Repo     = "opencv/palm_detection_mediapipe"
        Revision = "233e619dcea1759bf6de707b9b904fe30881ea55"
        Sha256   = "78ff51c38496b7fc8b8ebdb6cc8c1abb02fa6c38427c6848254cdaba57fcce7c"
    },
    @{
        Name     = "handpose_estimation_mediapipe_2023feb.onnx"
        Repo     = "opencv/handpose_estimation_mediapipe"
        Revision = "4b2a0b446e5cf2f11fb6b2c7251091c035d2c1f7"
        Sha256   = "db0898ae717b76b075d9bf563af315b29562e11f8df5027a1ef07b02bef6d81c"
    }
)

$root = Resolve-Path (Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "..")
$notice = Join-Path $root "models\NOTICE.txt"
if (-not (Test-Path $notice)) { throw "models\NOTICE.txt is missing; the models do not ship without it." }

function Get-Hash([string]$Path) { (Get-FileHash -Algorithm SHA256 $Path).Hash.ToLowerInvariant() }

New-Item -ItemType Directory -Force -Path $Cache | Out-Null
New-Item -ItemType Directory -Force -Path $Destination | Out-Null

foreach ($model in $Models) {
    $url = "https://huggingface.co/$($model.Repo)/resolve/$($model.Revision)/$($model.Name)"
    foreach ($pin in @($url, $model.Sha256)) {
        if (-not (Select-String -Path $notice -SimpleMatch $pin -Quiet)) {
            throw "models\NOTICE.txt does not mention $pin; update it with the pins in this script."
        }
    }

    $cached = Join-Path $Cache "$($model.Revision)-$($model.Name)"
    if (-not (Test-Path $cached) -or (Get-Hash $cached) -ne $model.Sha256) {
        Write-Host "Downloading $url"
        $ProgressPreference = "SilentlyContinue"
        Invoke-WebRequest -Uri $url -OutFile $cached -UseBasicParsing
    }
    $actual = Get-Hash $cached
    if ($actual -ne $model.Sha256) {
        Remove-Item -Force $cached
        throw "$($model.Name) does not match the pinned SHA-256 (expected $($model.Sha256), got $actual). Not shipping it."
    }

    $target = Join-Path $Destination $model.Name
    Copy-Item -Force $cached $target
    if ((Get-Hash $target) -ne $model.Sha256) { throw "$target was not written intact." }
    Write-Host "Model $($model.Name) ($actual)"
}

# The notice travels with the models, except when the destination is the repository's own folder.
if ((Resolve-Path (Split-Path -Parent $notice)).Path -ne (Resolve-Path $Destination).Path) {
    Copy-Item -Force $notice (Join-Path $Destination "NOTICE.txt")
}
