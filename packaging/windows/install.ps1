# Installs MauKode for the current user (no administrator rights needed).
# Uninstall: .\install.ps1 -Uninstall
param(
    [switch]$Uninstall,
    [string]$Prefix = (Join-Path $env:LOCALAPPDATA 'Programs\MauKode')
)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

function Remove-FromUserPath([string]$dir) {
    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $current) { return }
    $kept = $current -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ne $dir.TrimEnd('\')) }
    [Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')
}

if ($Uninstall) {
    $exe = Join-Path $Prefix 'maukode.exe'
    if (Test-Path $exe) { try { & $exe server stop *> $null } catch {} }
    # A running daemon keeps maukode.exe locked, so stop it before deleting.
    Remove-Item -Recurse -Force $Prefix -ErrorAction SilentlyContinue
    Remove-FromUserPath $Prefix
    Write-Host 'removed'
    exit 0
}

$old = Join-Path $Prefix 'maukode.exe'
if (Test-Path $old) { try { & $old server stop *> $null } catch {} }

New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
Copy-Item -Force (Join-Path $here 'maukode.exe') $Prefix
$web = Join-Path $Prefix 'web'
if (Test-Path $web) { Remove-Item -Recurse -Force $web }
Copy-Item -Recurse -Force (Join-Path $here 'web') $web

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$onPath = $userPath -and (($userPath -split ';') | Where-Object { $_.TrimEnd('\') -eq $Prefix.TrimEnd('\') })
if (-not $onPath) {
    [Environment]::SetEnvironmentVariable('Path', (($userPath, $Prefix | Where-Object { $_ }) -join ';'), 'User')
    Write-Host "added $Prefix to your user PATH (open a new terminal to pick it up)"
}

Write-Host "installed to $Prefix"
if (-not (Get-Command bash -ErrorAction SilentlyContinue) -and
    -not (Test-Path (Join-Path $env:ProgramFiles 'Git\bin\bash.exe'))) {
    Write-Warning 'Git for Windows was not found. The bash tool and hooks need its bash.exe; install it from https://git-scm.com/download/win'
}
Write-Host 'start the daemon (the web UI is served automatically):'
Write-Host '  maukode server start'
Write-Host 'then open the address it prints.'
