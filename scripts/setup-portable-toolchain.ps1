# Project-local build prerequisites; does not install software system-wide.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$toolsDirectory = Join-Path $projectRoot '.tools'
New-Item -ItemType Directory -Path $toolsDirectory -Force | Out-Null
$release = Invoke-RestMethod 'https://api.github.com/repos/mstorsjo/llvm-mingw/releases/tags/20261104'
$asset = $release.assets | Where-Object { $_.name -eq 'llvm-mingw-20261104-ucrt-x86_64.zip' } | Select-Object -First 1
if (!$asset) { throw 'Pinned official LLVM-MinGW asset unavailable' }
$archive = Join-Path $toolsDirectory $asset.name
if (!(Test-Path -LiteralPath $archive)) { Invoke-WebRequest $asset.browser_download_url -OutFile $archive }
if ($asset.digest -match '^sha256:(.+)$') {
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $Matches[1]) { throw 'LLVM-MinGW SHA256 mismatch' }
}
Expand-Archive -LiteralPath $archive -DestinationPath $toolsDirectory -Force
$cargoDirectory = Join-Path $env:USERPROFILE '.cargo/bin'
& (Join-Path $cargoDirectory 'rustup.exe') target add x86_64-pc-windows-gnullvm
if ($LASTEXITCODE) { throw 'Rust target installation failed' }
# A GNU-host Rust installation also needs its own GCC unwind support when it
# links build scripts/proc macros using the LLVM-MinGW GCC-compatible driver.
$hostSysroot = & (Join-Path $cargoDirectory 'rustc.exe') --print sysroot
$hostLibraries = Join-Path $hostSysroot 'lib/rustlib/x86_64-pc-windows-gnu/lib/self-contained'
if (Test-Path -LiteralPath $hostLibraries) {
    $compilerLibraries = Join-Path $toolsDirectory 'llvm-mingw-20261104-ucrt-x86_64/x86_64-w64-mingw32/lib'
    Copy-Item -LiteralPath (Join-Path $hostLibraries 'libgcc.a'),(Join-Path $hostLibraries 'libgcc_eh.a') -Destination $compilerLibraries
}
Write-Output 'Portable compiler is ready. Run ./scripts/build.ps1 -Toolchain gnullvm -Test'
