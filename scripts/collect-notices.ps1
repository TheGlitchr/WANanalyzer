param([string]$Target = 'x86_64-pc-windows-gnullvm')
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    $metadataText = & cargo metadata --locked --format-version 1 --filter-platform $Target
    if ($LASTEXITCODE) { throw 'Unable to read Cargo metadata' }
    $metadata = $metadataText | ConvertFrom-Json
    $resolvedIds = @{}
    foreach ($node in $metadata.resolve.nodes) { $resolvedIds[$node.id] = $true }
    $notices = [System.Text.StringBuilder]::new()
    [void]$notices.AppendLine('WANanalyzer — Third-party component notices')
    [void]$notices.AppendLine('Generated from Cargo.lock and packaged dependency license files.')
    foreach ($package in ($metadata.packages | Where-Object { $resolvedIds.ContainsKey($_.id) -and $_.name -ne 'wanalyzer' } | Sort-Object name,version)) {
        [void]$notices.AppendLine("`n============================================================")
        [void]$notices.AppendLine(($package.name + ' ' + $package.version + ' — ' + $package.license))
        [void]$notices.AppendLine($package.repository)
        $packageDirectory = Split-Path -Parent $package.manifest_path
        $licenseFiles = Get-ChildItem -LiteralPath $packageDirectory -File -Recurse -Depth 3 | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE|OFL)([-._ ].*)?$' }
        foreach ($licenseFile in $licenseFiles) {
            [void]$notices.AppendLine("`n" + $licenseFile.Name)
            [void]$notices.AppendLine((Get-Content -LiteralPath $licenseFile.FullName -Raw))
        }
        if (!$licenseFiles) { [void]$notices.AppendLine('No packaged license text found; consult the source repository and declared SPDX license before public distribution.') }
    }
    foreach ($sdkFile in @('LICENSE.txt','NOTICE.txt')) {
        [void]$notices.AppendLine("`nMicrosoft WebView2 loader 1.0.3800.47 — " + $sdkFile)
        [void]$notices.AppendLine((Get-Content -LiteralPath (Join-Path $projectRoot ('assets/webview2-notices/' + $sdkFile)) -Raw))
    }
    [void]$notices.AppendLine("`nRust standard library: MIT OR Apache-2.0; https://github.com/rust-lang/rust/tree/master/LICENSES")
    [void]$notices.AppendLine('LLVM compiler runtime/unwinder: Apache-2.0 WITH LLVM-exception; https://github.com/llvm/llvm-project/blob/main/LICENSE.TXT')
    [void]$notices.AppendLine((Get-Content -LiteralPath (Join-Path $projectRoot 'assets/iperf3/NOTICES.txt') -Raw))
    [System.IO.File]::WriteAllText((Join-Path $projectRoot 'assets/THIRD_PARTY_NOTICES.txt'),$notices.ToString(),[System.Text.UTF8Encoding]::new($false))
    Write-Output 'Generated assets/THIRD_PARTY_NOTICES.txt'
} finally { Pop-Location }
