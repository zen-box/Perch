param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][ValidateSet('x86_64', 'aarch64')][string]$Architecture,
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$OutputDir
)
$ErrorActionPreference = 'Stop'
if ($Version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') { throw 'Invalid version' }
if (!(Test-Path -LiteralPath $Binary -PathType Leaf)) { throw "Missing native binary: $Binary" }
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$icon = Join-Path $repo 'assets/perch.ico'
if (!(Test-Path -LiteralPath $icon -PathType Leaf)) { throw "Missing icon: $icon" }
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$env:PERCH_VERSION = $Version
$env:PERCH_ARCH = $Architecture
$env:PERCH_BINARY = (Resolve-Path -LiteralPath $Binary).Path
$env:PERCH_ICON = (Resolve-Path -LiteralPath $icon).Path
$env:PERCH_OUTPUT = (Resolve-Path -LiteralPath $OutputDir).Path
$candidates = @(
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 7/ISCC.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe'),
    (Join-Path $env:ProgramFiles 'Inno Setup 7/ISCC.exe'),
    (Join-Path $env:ProgramFiles 'Inno Setup 6/ISCC.exe')
)
$inno = $candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if (!$inno) { throw 'Inno Setup ISCC.exe not found after installation' }
& $inno (Join-Path $PSScriptRoot 'perch.iss')
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed: $LASTEXITCODE" }
$installer = Join-Path $env:PERCH_OUTPUT "Perch-$Version-windows-$Architecture-setup.exe"
if (!(Test-Path -LiteralPath $installer -PathType Leaf) -or (Get-Item -LiteralPath $installer).Length -eq 0) {
    throw "Installer missing or empty: $installer"
}
Write-Output $installer
