# Rebuild the Windows icon from the same vector geometry as assets/wanalyzer.svg.
# Uses only Windows' built-in System.Drawing; no external asset or font dependency.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$projectRoot = Split-Path -Parent $PSScriptRoot
$assetDirectory = Join-Path $projectRoot 'assets'
$sizes = @(16,24,32,48,64,128,256)

function New-RoundedTile([single]$x,[single]$y,[single]$width,[single]$radius) {
    $path = [System.Drawing.Drawing2D.GraphicsPath]::new()
    $diameter = $radius * 2
    $path.AddArc($x,$y,$diameter,$diameter,180,90)
    $path.AddArc($x+$width-$diameter,$y,$diameter,$diameter,270,90)
    $path.AddArc($x+$width-$diameter,$y+$width-$diameter,$diameter,$diameter,0,90)
    $path.AddArc($x,$y+$width-$diameter,$diameter,$diameter,90,90)
    $path.CloseFigure()
    return ,$path
}

function New-IconBitmap([int]$size) {
    $supersample = 4
    $large = [System.Drawing.Bitmap]::new($size*$supersample,$size*$supersample,[System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [System.Drawing.Graphics]::FromImage($large)
    $graphics.Clear([System.Drawing.Color]::Transparent)
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.ScaleTransform(($size*$supersample/256.0),($size*$supersample/256.0))
    $tile = New-RoundedTile 8 8 240 54
    $gradient = [System.Drawing.Drawing2D.LinearGradientBrush]::new([System.Drawing.Point]::new(8,8),[System.Drawing.Point]::new(248,248),[System.Drawing.ColorTranslator]::FromHtml('#24374c'),[System.Drawing.ColorTranslator]::FromHtml('#101827'))
    $graphics.FillPath($gradient,$tile)
    $border = New-RoundedTile 10 10 236 52
    $edge = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(128,83,122,149),2)
    $graphics.DrawPath($edge,$border)
    $orbit = [System.Drawing.Pen]::new([System.Drawing.ColorTranslator]::FromHtml('#578bc1'),8)
    $orbit.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $orbit.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $graphics.DrawArc($orbit,44,44,168,168,25,280)
    $signalBrush = [System.Drawing.Drawing2D.LinearGradientBrush]::new([System.Drawing.Point]::new(49,128),[System.Drawing.Point]::new(207,128),[System.Drawing.ColorTranslator]::FromHtml('#50dcaa'),[System.Drawing.ColorTranslator]::FromHtml('#68e8f4'))
    $signal = [System.Drawing.Pen]::new($signalBrush,18)
    $signal.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $signal.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $signal.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
    $points = [System.Drawing.PointF[]]@([System.Drawing.PointF]::new(49,103),[System.Drawing.PointF]::new(84,166),[System.Drawing.PointF]::new(126,91),[System.Drawing.PointF]::new(168,166),[System.Drawing.PointF]::new(207,84))
    $graphics.DrawLines($signal,$points)
    $nodes = @(@(49,103,11,'#d9fff0'),@(207,84,12,'#c8f9ff'),@(207,84,5,'#338cab'),@(176.18,59.19,7,'#88baff'))
    foreach ($node in $nodes) {
        $brush = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml($node[3]))
        $graphics.FillEllipse($brush,[single]($node[0]-$node[2]),[single]($node[1]-$node[2]),[single]($node[2]*2),[single]($node[2]*2))
        $brush.Dispose()
    }
    $graphics.Dispose()
    foreach ($resource in @($tile,$gradient,$border,$edge,$orbit,$signalBrush,$signal)) { $resource.Dispose() }
    $bitmap = [System.Drawing.Bitmap]::new($size,$size,[System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $graphics.DrawImage($large,[System.Drawing.Rectangle]::new(0,0,$size,$size))
    $graphics.Dispose(); $large.Dispose()
    return ,$bitmap
}

$frames = @()
foreach ($size in $sizes) {
    $bitmap = New-IconBitmap $size
    $stream = [System.IO.MemoryStream]::new()
    if ($size -eq 256) {
        $bitmap.Save((Join-Path $assetDirectory 'wanalyzer.png'),[System.Drawing.Imaging.ImageFormat]::Png)
        $bitmap.Save($stream,[System.Drawing.Imaging.ImageFormat]::Png)
    } else {
        # Classic 32-bit DIB frames plus an AND mask for native small-icon support.
        $writer = [System.IO.BinaryWriter]::new($stream,[System.Text.Encoding]::UTF8,$true)
        $maskStride = [int]([Math]::Ceiling($size/32.0)*4)
        $writer.Write([uint32]40); $writer.Write([int32]$size); $writer.Write([int32]($size*2))
        $writer.Write([uint16]1); $writer.Write([uint16]32); $writer.Write([uint32]0)
        $writer.Write([uint32]($size*$size*4)); $writer.Write([int32]0); $writer.Write([int32]0)
        $writer.Write([uint32]0); $writer.Write([uint32]0)
        for ($y=$size-1; $y -ge 0; $y--) {
            for ($x=0; $x -lt $size; $x++) {
                $pixel = $bitmap.GetPixel($x,$y)
                $writer.Write([byte]$pixel.B); $writer.Write([byte]$pixel.G); $writer.Write([byte]$pixel.R); $writer.Write([byte]$pixel.A)
            }
        }
        for ($y=$size-1; $y -ge 0; $y--) {
            $mask = [byte[]]::new($maskStride)
            for ($x=0; $x -lt $size; $x++) {
                if ($bitmap.GetPixel($x,$y).A -eq 0) { $mask[[int][Math]::Floor($x/8.0)] = $mask[[int][Math]::Floor($x/8.0)] -bor (128 -shr ($x%8)) }
            }
            $writer.Write($mask)
        }
        $writer.Dispose()
    }
    $frames += @{ Size=$size; Bytes=$stream.ToArray() }
    $stream.Dispose(); $bitmap.Dispose()
}
$file = [System.IO.File]::Create((Join-Path $assetDirectory 'wanalyzer.ico'))
$output = [System.IO.BinaryWriter]::new($file)
try {
    $output.Write([uint16]0); $output.Write([uint16]1); $output.Write([uint16]$frames.Count)
    $offset = 6 + 16*$frames.Count
    foreach ($frame in $frames) {
        $dimension = if ($frame.Size -eq 256) {0} else {$frame.Size}
        $output.Write([byte]$dimension); $output.Write([byte]$dimension)
        $output.Write([byte]0); $output.Write([byte]0); $output.Write([uint16]1); $output.Write([uint16]32)
        $output.Write([uint32]$frame.Bytes.Length); $output.Write([uint32]$offset)
        $offset += $frame.Bytes.Length
    }
    foreach ($frame in $frames) { $output.Write([byte[]]$frame.Bytes) }
} finally { $output.Dispose() }
Write-Output 'Generated assets/wanalyzer.png and assets/wanalyzer.ico (16, 24, 32, 48, 64, 128, 256 px).'
