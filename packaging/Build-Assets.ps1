param([string]$AssetRoot = (Join-Path $PSScriptRoot '..\assets'))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$AssetRoot = [IO.Path]::GetFullPath($AssetRoot)
New-Item -ItemType Directory -Path $AssetRoot -Force | Out-Null
function New-NearBitmap([int]$Size) {
    $bitmap = [Drawing.Bitmap]::new($Size,$Size)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $graphics.ScaleTransform($Size / 32.0,$Size / 32.0)
        $surface = [Drawing.Drawing2D.GraphicsPath]::new()
        $surface.AddArc(1,1,16,16,180,90); $surface.AddArc(15,1,16,16,270,90)
        $surface.AddArc(15,15,16,16,0,90); $surface.AddArc(1,15,16,16,90,90); $surface.CloseFigure()
        $accent = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(35,103,89))
        $white=[Drawing.SolidBrush]::new([Drawing.Color]::White)
        $graphics.FillPath($accent,$surface)
        $graphics.FillEllipse($white,7,7,9,9)
        $graphics.FillRectangle($white,11,11,4,12)
        $graphics.FillEllipse($white,11,21,4,4)
        $graphics.FillEllipse($white,18,6,8,8)
        $graphics.FillRectangle($white,18,10,4,11)
        $graphics.FillEllipse($white,18,19,4,4)
        $graphics.FillEllipse($accent,8,9,3,4)
        $graphics.FillEllipse($accent,22,8,2,3)
        $white.Dispose(); $accent.Dispose(); $surface.Dispose()
    } finally { $graphics.Dispose() }
    return $bitmap
}
$iconSizes = @(16,24,32,48,64,256)
$iconImages = [Collections.Generic.List[byte[]]]::new()
foreach ($size in $iconSizes) {
    $bitmap = New-NearBitmap $size
    $stream = [IO.MemoryStream]::new()
    try { $bitmap.Save($stream,[Drawing.Imaging.ImageFormat]::Png); $iconImages.Add($stream.ToArray()) }
    finally { $bitmap.Dispose(); $stream.Dispose() }
}
$iconStream = [IO.File]::Create((Join-Path $AssetRoot 'near.ico'))
$writer = [IO.BinaryWriter]::new($iconStream)
try {
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$iconSizes.Count)
    $offset = 6 + 16 * $iconSizes.Count
    for ($index=0; $index -lt $iconSizes.Count; $index++) {
        $dimension = if ($iconSizes[$index] -eq 256) { 0 } else { $iconSizes[$index] }
        $writer.Write([byte]$dimension); $writer.Write([byte]$dimension); $writer.Write([byte]0); $writer.Write([byte]0)
        $writer.Write([uint16]1); $writer.Write([uint16]32); $writer.Write([uint32]$iconImages[$index].Length); $writer.Write([uint32]$offset)
        $offset += $iconImages[$index].Length
    }
    foreach($bytes in $iconImages) { $writer.Write($bytes) }
} finally { $writer.Dispose() }
