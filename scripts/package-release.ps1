param([string]$Version = '', [string]$Executable = '')
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (!$Version) { $Version = [regex]::Match((Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -Raw), '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value }
if ($Version -notmatch '^\d+\.\d+\.\d+([.-][A-Za-z0-9.-]+)?$') { throw 'Invalid release version' }
if (!$Executable) { $Executable = Join-Path $projectRoot 'dist/WANanalyzer.exe' }
if (!(Test-Path -LiteralPath $Executable -PathType Leaf)) { throw 'Build dist/WANanalyzer.exe first, or pass -Executable' }
# Use a fresh staging directory so existing runtime data can never enter the ZIP.
$staging = Join-Path $projectRoot ('artifacts/release-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging -Force | Out-Null
Copy-Item -LiteralPath $Executable -Destination (Join-Path $staging 'WANanalyzer.exe')
foreach ($document in @('LICENSE','README.md','CHANGELOG.md','VALIDATION.md','NATIVE_SPEED_TESTS.md','IPERF.md','PRD.md','TECHNICAL_RESEARCH.md')) {
    Copy-Item -LiteralPath (Join-Path $projectRoot $document) -Destination $staging
}
foreach ($image in @('assets/wanalyzer.png','docs/images/monitor.png')) {
    $destination = Join-Path $staging $image
    New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot $image) -Destination $destination
}
New-Item -ItemType Directory -Path (Join-Path $staging 'assets/webview2-notices') -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot 'assets/THIRD_PARTY_NOTICES.txt') -Destination (Join-Path $staging 'assets')
foreach ($notice in @('LICENSE.txt','NOTICE.txt')) {
    Copy-Item -LiteralPath (Join-Path $projectRoot "assets/webview2-notices/$notice") -Destination (Join-Path $staging 'assets/webview2-notices')
}
$archive = Join-Path $projectRoot "dist/WANanalyzer-$Version-windows-x64.zip"
# Corresponding source and licenses for the embedded iPerf/Cygwin runtime.
New-Item -ItemType Directory -Path (Join-Path $staging 'assets/iperf3/sources') -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot 'assets/iperf3/NOTICES.txt') -Destination (Join-Path $staging 'assets/iperf3')
foreach ($source in @('iperf-3.22.tar.gz','cygwin-3.6.10-1-src.tar.xz')) {
    Copy-Item -LiteralPath (Join-Path $projectRoot "assets/iperf3/sources/$source") -Destination (Join-Path $staging 'assets/iperf3/sources')
}
Compress-Archive -Path (Join-Path $staging '*') -DestinationPath $archive -Force
$hashLines = @(
    ((Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash.ToLowerInvariant() + '  WANanalyzer.exe')
    ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + (Split-Path -Leaf $archive))
)
[System.IO.File]::WriteAllLines((Join-Path $projectRoot 'dist/SHA256SUMS.txt'), $hashLines, [System.Text.UTF8Encoding]::new($false))
Write-Output $archive
