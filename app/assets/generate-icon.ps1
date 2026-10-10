<#
.SYNOPSIS
    Generate the Mayasaba application icon (.ico) and reference PNGs.

.DESCRIPTION
    Rasterises the original "Three Sightlines, One Node" mark — the same geometry that
    app/Mayasaba.App/Theme/Styles.xaml carries as MayasabaMarkLines / MayasabaMarkNode — with
    System.Drawing only. No network access, no external tools, no NuGet.

    Writes:
      app/assets/mayasaba.ico          multi-resolution icon (16,20,24,32,40,48,64,128,256),
                                       PNG-compressed frames (Vista and later)
      app/assets/icons/mayasaba-N.png  one reference PNG per size

    The .ico is consumed twice: embedded into Mayasaba.App.exe by app/Mayasaba.App/app.rc, and
    copied next to the executable so the window can call AppWindow.SetIcon at runtime.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File app/assets/generate-icon.ps1
#>
[CmdletBinding()]
param(
    [string] $OutputDirectory = '',
    [string] $IcoPath = '',
    [string] $PlateColor = '#4057C8',
    [string] $MarkColor = '#FFFFFF'
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

# $PSScriptRoot is not reliably populated inside param() defaults on Windows PowerShell 5.1, so
# resolve the default paths here instead.
$scriptRoot = $PSScriptRoot
if ([string]::IsNullOrEmpty($scriptRoot)) {
    $scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
}
if ([string]::IsNullOrEmpty($OutputDirectory)) {
    $OutputDirectory = Join-Path $scriptRoot 'icons'
}
if ([string]::IsNullOrEmpty($IcoPath)) {
    $IcoPath = Join-Path $scriptRoot 'mayasaba.ico'
}

# --- Geometry: identical to Theme/Styles.xaml (64x64 design grid) -----------------------------
$GridSize     = 64.0
$CornerRadius = 14.0
$StrokeWidth  = 4.5
$NodeCenterX  = 32.0
$NodeCenterY  = 30.0
$NodeRadius   = 5.2

# x1, y1, x2, y2 — the three gapped sightlines and the plumb baseline.
$Segments = @(
    @(  9.0, 10.0, 26.5, 24.5),
    @( 55.0, 10.0, 37.5, 24.5),
    @( 32.0, 58.0, 32.0, 37.5),
    @( 22.0, 58.0, 42.0, 58.0)
)

function New-RoundedRectPath {
    param([single] $X, [single] $Y, [single] $Width, [single] $Height, [single] $Radius)
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $diameter = $Radius * 2.0
    $path.AddArc($X, $Y, $diameter, $diameter, 180, 90)
    $path.AddArc($X + $Width - $diameter, $Y, $diameter, $diameter, 270, 90)
    $path.AddArc($X + $Width - $diameter, $Y + $Height - $diameter, $diameter, $diameter, 0, 90)
    $path.AddArc($X, $Y + $Height - $diameter, $diameter, $diameter, 90, 90)
    $path.CloseFigure()
    return $path
}

function New-MarkBitmap {
    param([int] $Size)

    $bitmap = New-Object System.Drawing.Bitmap($Size, $Size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $graphics.Clear([System.Drawing.Color]::Transparent)

        $scale = [single]($Size / $GridSize)

        # Authority plate
        $plateBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml($PlateColor))
        $platePath = New-RoundedRectPath -X (2.0 * $scale) -Y (2.0 * $scale) `
                                         -Width (60.0 * $scale) -Height (60.0 * $scale) `
                                         -Radius ([single]($CornerRadius * $scale))
        $graphics.FillPath($plateBrush, $platePath)
        $platePath.Dispose()
        $plateBrush.Dispose()

        # Sightlines
        $markColor = [System.Drawing.ColorTranslator]::FromHtml($MarkColor)
        $pen = New-Object System.Drawing.Pen($markColor, [single]($StrokeWidth * $scale))
        $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
        $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
        $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
        foreach ($segment in $Segments) {
            $graphics.DrawLine($pen,
                [single]($segment[0] * $scale), [single]($segment[1] * $scale),
                [single]($segment[2] * $scale), [single]($segment[3] * $scale))
        }
        $pen.Dispose()

        # The single node of verified project reality
        $nodeBrush = New-Object System.Drawing.SolidBrush($markColor)
        $radius = [single]($NodeRadius * $scale)
        $graphics.FillEllipse($nodeBrush,
            [single](($NodeCenterX - $NodeRadius) * $scale),
            [single](($NodeCenterY - $NodeRadius) * $scale),
            $radius * 2.0, $radius * 2.0)
        $nodeBrush.Dispose()
    }
    finally {
        $graphics.Dispose()
    }
    return $bitmap
}

function Write-IcoFile {
    param([string] $Path, [object[]] $Frames)

    $stream = New-Object System.IO.MemoryStream
    $writer = New-Object System.IO.BinaryWriter($stream)
    try {
        $writer.Write([UInt16]0)               # reserved
        $writer.Write([UInt16]1)               # 1 = icon
        $writer.Write([UInt16]$Frames.Count)

        $offset = 6 + (16 * $Frames.Count)
        foreach ($frame in $Frames) {
            # 256 is encoded as 0 in the directory entry.
            $dimension = if ($frame.Size -ge 256) { [Byte]0 } else { [Byte]$frame.Size }
            $writer.Write($dimension)          # width
            $writer.Write($dimension)          # height
            $writer.Write([Byte]0)             # palette count
            $writer.Write([Byte]0)             # reserved
            $writer.Write([UInt16]1)           # colour planes
            $writer.Write([UInt16]32)          # bits per pixel
            $writer.Write([UInt32]$frame.Bytes.Length)
            $writer.Write([UInt32]$offset)
            $offset += $frame.Bytes.Length
        }
        foreach ($frame in $Frames) {
            $writer.Write($frame.Bytes)
        }
        $writer.Flush()
    }
    finally {
        $writer.Dispose()
    }

    [System.IO.File]::WriteAllBytes($Path, $stream.ToArray())
    $stream.Dispose()
}

# --- Emit -------------------------------------------------------------------------------------
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$iconDirectory = Split-Path -Parent $IcoPath
if ($iconDirectory) {
    New-Item -ItemType Directory -Force -Path $iconDirectory | Out-Null
}

$sizes = @(16, 20, 24, 32, 40, 48, 64, 128, 256)
$frames = @()

foreach ($size in $sizes) {
    $bitmap = New-MarkBitmap -Size $size
    try {
        $pngPath = Join-Path $OutputDirectory ('mayasaba-{0}.png' -f $size)
        $bitmap.Save($pngPath, [System.Drawing.Imaging.ImageFormat]::Png)

        $frameStream = New-Object System.IO.MemoryStream
        $bitmap.Save($frameStream, [System.Drawing.Imaging.ImageFormat]::Png)
        $frames += [pscustomobject]@{ Size = $size; Bytes = $frameStream.ToArray() }
        $frameStream.Dispose()
    }
    finally {
        $bitmap.Dispose()
    }
}

Write-IcoFile -Path $IcoPath -Frames $frames

Write-Host ('[generate-icon] wrote {0} ({1} bytes, {2} frames: {3})' -f `
    $IcoPath, (Get-Item $IcoPath).Length, $frames.Count, ($sizes -join ', '))
Write-Host ('[generate-icon] reference PNGs in {0}' -f $OutputDirectory)
