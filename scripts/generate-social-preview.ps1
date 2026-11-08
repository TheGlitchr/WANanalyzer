# Compose the repository preview from the existing EXE artwork.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$projectRoot = Split-Path -Parent $PSScriptRoot
$outputDirectory = Join-Path $projectRoot 'docs/images'
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
$canvas = [System.Drawing.Bitmap]::new(1280,640)
$graphics = [System.Drawing.Graphics]::FromImage($canvas)
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$graphics.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
$background = [System.Drawing.Drawing2D.LinearGradientBrush]::new(
    [System.Drawing.Point]::new(0,0), [System.Drawing.Point]::new(1280,640),
    [System.Drawing.ColorTranslator]::FromHtml('#182838'),
    [System.Drawing.ColorTranslator]::FromHtml('#0d1420'))
$mint = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml('#64dfbb'))
$white = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml('#edf4fc'))
$muted = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml('#a5b8cc'))
$line = [System.Drawing.Pen]::new([System.Drawing.ColorTranslator]::FromHtml('#30465b'),1)
$accent = [System.Drawing.Pen]::new([System.Drawing.ColorTranslator]::FromHtml('#64dfbb'),4)
$title = [System.Drawing.Font]::new('Segoe UI',64,[System.Drawing.FontStyle]::Bold,[System.Drawing.GraphicsUnit]::Pixel)
$body = [System.Drawing.Font]::new('Segoe UI',28,[System.Drawing.FontStyle]::Regular,[System.Drawing.GraphicsUnit]::Pixel)
$small = [System.Drawing.Font]::new('Segoe UI',20,[System.Drawing.FontStyle]::Regular,[System.Drawing.GraphicsUnit]::Pixel)
$icon = [System.Drawing.Image]::FromFile((Join-Path $projectRoot 'assets/wanalyzer.png'))
try {
    $graphics.FillRectangle($background,0,0,1280,640)
    $graphics.DrawString('WINDOWS 10 / 11   |   PORTABLE EXE',$small,$mint,80,62)
    $graphics.DrawImage($icon,80,180,256,256)
    $graphics.DrawString('WANanalyzer',$title,$white,386,204)
    $graphics.DrawString('See what your connection is doing.',$body,$muted,390,292)
    $graphics.DrawString('Latency. Packet loss. Speed tests.',$body,$white,390,346)
    $graphics.DrawLine($accent,80,508,180,508)
    $graphics.DrawLine($line,180,508,1200,508)
    $graphics.DrawString('Continuous monitoring  /  Per-hop loss  /  Loaded latency',$small,$muted,80,538)
    $canvas.Save((Join-Path $outputDirectory 'social-preview.png'),[System.Drawing.Imaging.ImageFormat]::Png)
} finally {
    foreach ($resource in @($icon,$title,$body,$small,$line,$accent,$mint,$white,$muted,$background,$graphics,$canvas)) { $resource.Dispose() }
}
Write-Output 'Generated docs/images/social-preview.png (1280 x 640).'
