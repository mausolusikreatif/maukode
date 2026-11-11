# MauKode installer (Windows x86_64). Downloads a release zip from GitHub, verifies its checksum,
# and installs the daemon and web UI for the current user (no administrator rights needed).
#
#   irm https://raw.githubusercontent.com/mausolusikreatif/maukode/master/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/mausolusikreatif/maukode/master/install.ps1))) -Version v0.7.2
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/mausolusikreatif/maukode/master/install.ps1))) -Uninstall
param(
    [string]$Version = '',
    [switch]$Uninstall
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$repo = 'mausolusikreatif/maukode'

if ($Uninstall) {
    $prefix = Join-Path $env:LOCALAPPDATA 'Programs\MauKode'
    $exe = Join-Path $prefix 'maukode.exe'
    if (Test-Path $exe) { try { & $exe server stop *> $null } catch {} }
    Remove-Item -Recurse -Force $prefix -ErrorAction SilentlyContinue
    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($current) {
        $kept = $current -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ne $prefix) }
        [Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')
    }
    Write-Host 'removed'
    return
}

if (-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
    throw 'only Windows x86_64 release builds exist; build from source instead (see README)'
}

if (-not $Version) {
    # /releases/latest redirects to /releases/tag/<tag>; no API token or rate limit involved.
    $req = [System.Net.HttpWebRequest]::Create("https://github.com/$repo/releases/latest")
    $req.AllowAutoRedirect = $false
    $resp = $req.GetResponse()
    $location = $resp.Headers['Location']
    $resp.Close()
    $Version = ($location -split '/')[-1]
}
if ($Version -notmatch '^v\d') { throw "could not resolve a release version (got '$Version')" }

$name = "maukode-windows-x86_64-$Version"
$base = "https://github.com/$repo/releases/download/$Version"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("maukode-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
    Write-Host "downloading MauKode $Version"
    Invoke-WebRequest -UseBasicParsing "$base/$name.zip" -OutFile "$tmp\$name.zip"
    Invoke-WebRequest -UseBasicParsing "$base/$name.zip.sha256" -OutFile "$tmp\$name.zip.sha256"
    $expected = ((Get-Content "$tmp\$name.zip.sha256" -Raw).Trim() -split '\s+')[0].ToLower()
    $actual = (Get-FileHash "$tmp\$name.zip" -Algorithm SHA256).Hash.ToLower()
    if ($expected -ne $actual) { throw "checksum mismatch for $name.zip; nothing was installed" }
    Write-Host 'checksum ok'

    Expand-Archive -Path "$tmp\$name.zip" -DestinationPath $tmp -Force
    & powershell -NoProfile -ExecutionPolicy Bypass -File "$tmp\$name\install.ps1"
    if ($LASTEXITCODE) { throw "the installer failed (exit code $LASTEXITCODE)" }
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
