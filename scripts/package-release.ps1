param([string]$Version = '0.1.0')
$ErrorActionPreference = 'Stop'
if ($Version -notmatch '^\d+\.\d+\.\d+([.-][A-Za-z0-9.-]+)?$') { throw 'Invalid release version' }
$projectRoot = Split-Path -Parent $PSScriptRoot
$executable = Join-Path $projectRoot 'dist/WANanalyzer.exe'
if (!(Test-Path -LiteralPath $executable -PathType Leaf)) { throw 'Build dist/WANanalyzer.exe first' }
# Use a fresh staging directory so existing runtime data can never enter the ZIP.
$staging = Join-Path $projectRoot ('artifacts/release-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging -Force | Out-Null
Copy-Item -LiteralPath $executable -Destination (Join-Path $staging 'WANanalyzer.exe')
foreach ($document in @('LICENSE','README.md','CHANGELOG.md','VALIDATION.md','NATIVE_SPEED_TESTS.md','PRD.md','TECHNICAL_RESEARCH.md')) {
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
Compress-Archive -Path (Join-Path $staging '*') -DestinationPath $archive -Force
$hashLines = foreach ($file in @($executable,$archive)) {
    (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + (Split-Path -Leaf $file)
}
[System.IO.File]::WriteAllLines((Join-Path $projectRoot 'dist/SHA256SUMS.txt'), $hashLines, [System.Text.UTF8Encoding]::new($false))
Write-Output $archive
