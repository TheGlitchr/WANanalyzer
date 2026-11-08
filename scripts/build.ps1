param(
    [ValidateSet('msvc','gnullvm')][string]$Toolchain = 'msvc',
    [switch]$Debug,
    [switch]$Test
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$previousPath = $env:Path
$previousCompiler = $env:CC
$previousArchiver = $env:AR
Push-Location -LiteralPath $projectRoot
try {
    $cargoDirectory = Join-Path $env:USERPROFILE '.cargo/bin'
    $env:Path = $cargoDirectory + ';' + $env:Path
    if ($Toolchain -eq 'gnullvm') {
        $compilerFolder = Get-ChildItem -LiteralPath (Join-Path $projectRoot '.tools') -Directory -Filter 'llvm-mingw-*-ucrt-x86_64' | Sort-Object Name -Descending | Select-Object -First 1
        if (!$compilerFolder) { throw 'LLVM-MinGW not found. Run scripts/setup-portable-toolchain.ps1 first.' }
        $env:Path = (Join-Path $compilerFolder.FullName 'bin') + ';' + $env:Path
        $env:CC = 'x86_64-w64-mingw32-clang'
        $env:AR = 'llvm-ar'
        $targetTriple = 'x86_64-pc-windows-gnullvm'
    } else { $targetTriple = 'x86_64-pc-windows-msvc' }
    if ($Test) { & cargo test --locked --target $targetTriple; if ($LASTEXITCODE) { throw 'Tests failed' } }
    $buildArgs = @('build','--locked','--target',$targetTriple)
    if (!$Debug) { $buildArgs += '--release' }
    & cargo @buildArgs
    if ($LASTEXITCODE) { throw 'Build failed' }
    $profileName = if ($Debug) { 'debug' } else { 'release' }
    New-Item -ItemType Directory -Path 'dist' -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot "target/$targetTriple/$profileName/wanalyzer.exe") -Destination 'dist/WANanalyzer.exe' -Force
    Get-FileHash -LiteralPath 'dist/WANanalyzer.exe' -Algorithm SHA256
} finally { $env:Path=$previousPath; $env:CC=$previousCompiler; $env:AR=$previousArchiver; Pop-Location }
