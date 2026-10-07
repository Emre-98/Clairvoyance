<#
.SYNOPSIS
  Measures recording settings on this PC's GPU encoder: captures ~60 s of your screen while you
  play (near-lossless reference), encodes that same capture with the current settings and with
  the candidates (quality-based rate control, HEVC, AV1, B-frames), and reports file size,
  VMAF, SSIM and PSNR for each. Nothing Riot is started: you start League yourself.

.DESCRIPTION
  Uses the ffmpeg that Clairvoyance downloads (Settings > Clips > Download ffmpeg; it has the
  NVIDIA / AMD / Intel encoders, desktop capture and VMAF) or one given with -Ffmpeg.
  Writes everything to <Videos>\Clairvoyance\perf-tests\encoder-compare-<date>\:
  results.txt (the table + what to switch), results.json, the reference and every candidate.
  The candidates use the GPU's own encoders through ffmpeg with the settings the recorder uses
  through Media Foundation (keyframe every second, no B-frames, low latency), so the numbers
  compare like with like.

.EXAMPLE
  .\scripts\encoder-compare.ps1
  # Start a Practice Tool game, run this, switch back to League within 10 s and play normally
  # for a minute (walk, fight, move the camera, open the shop once).

.EXAMPLE
  .\scripts\encoder-compare.ps1 -Reference D:\ref.mkv   # measure an existing capture again
#>
param(
  [int]$Seconds = 60,
  [string]$Reference = "",
  [string]$Ffmpeg = "",
  [string]$Out = "",
  [ValidateSet("auto", "nvenc", "amf", "qsv", "software")][string]$Vendor = "auto",
  [int]$Countdown = 10
)
$ErrorActionPreference = "Stop"

function Find-Ffmpeg {
  if ($Ffmpeg -and (Test-Path $Ffmpeg)) { return (Resolve-Path $Ffmpeg).Path }
  $roots = @()
  if ($env:LOCALAPPDATA) { $roots += (Join-Path $env:LOCALAPPDATA "Clairvoyance\ffmpeg") }
  foreach ($r in $roots) {
    if (Test-Path $r) {
      $f = Get-ChildItem -Path $r -Recurse -Filter "ffmpeg.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
      if ($f) { return $f.FullName }
    }
  }
  $c = Get-Command ffmpeg -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($c) { return $c.Source }
  throw "ffmpeg not found. In Clairvoyance: Settings > Clips > Download ffmpeg, or pass -Ffmpeg <path>."
}

$script:FF = Find-Ffmpeg
# ffmpeg writes its log to stderr; Windows PowerShell would treat that as an error.
function Invoke-FF([string[]]$FfArgs, [switch]$Quiet) {
  $old = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  try { $o = & $script:FF -hide_banner -nostdin -y @FfArgs 2>&1 } finally { $ErrorActionPreference = $old }
  $text = ($o | ForEach-Object { "$_" }) -join "`n"
  if ($LASTEXITCODE -ne 0 -and -not $Quiet) { throw "ffmpeg failed ($LASTEXITCODE): $($text.Substring([Math]::Max(0, $text.Length - 600)))" }
  return @{ ok = ($LASTEXITCODE -eq 0); log = $text }
}
function Test-Encoder([string]$Name) {
  (Invoke-FF @("-f", "lavfi", "-i", "color=black:s=1280x720:r=30", "-frames:v", "8", "-c:v", $Name, "-f", "null", "-") -Quiet).ok
}

if (-not $Out) {
  $videos = [Environment]::GetFolderPath("MyVideos")
  if (-not $videos) { $videos = Join-Path $HOME "Videos" }
  $Out = Join-Path $videos ("Clairvoyance\perf-tests\encoder-compare-" + (Get-Date -Format "yyyy-MM-dd_HH-mm"))
}
New-Item -ItemType Directory -Force -Path $Out | Out-Null
Write-Host "ffmpeg: $script:FF"
Write-Host "Output: $Out"
$filters = (Invoke-FF @("-filters") -Quiet).log
$hasVmaf = $filters -match "libvmaf"
if (-not $hasVmaf) { Write-Host "This ffmpeg has no VMAF: SSIM and PSNR only." -ForegroundColor Yellow }

# ---------- which encoders this GPU has ----------
if ($Vendor -eq "auto") {
  foreach ($v in @("nvenc", "amf", "qsv")) {
    if (Test-Encoder "h264_$v") { $Vendor = $v; break }
  }
  if ($Vendor -eq "auto") { $Vendor = "software" }
}
$enc = @{}
if ($Vendor -eq "software") {
  $enc.h264 = "libx264"; $enc.hevc = "libx265"; $enc.av1 = "libsvtav1"
} else {
  $enc.h264 = "h264_$Vendor"; $enc.hevc = "hevc_$Vendor"; $enc.av1 = "av1_$Vendor"
}
foreach ($k in @("hevc", "av1")) {
  if (-not (Test-Encoder $enc[$k])) { Write-Host "No $k encoder ($($enc[$k])) on this PC: skipped." -ForegroundColor Yellow; $enc.Remove($k) }
}
Write-Host "Encoders: $($enc.Values -join ', ') ($Vendor)"

# ---------- the reference: your screen at 1080p60, near-lossless ----------
$ref = $Reference
if (-not $ref) {
  $ref = Join-Path $Out "reference.mkv"
  Write-Host ""
  Write-Host "Switch to League now and play normally. Capturing $Seconds s in $Countdown s..." -ForegroundColor Cyan
  for ($i = $Countdown; $i -gt 0; $i--) { Write-Host -NoNewline "$i "; [Console]::Beep(800, 120); Start-Sleep -Milliseconds 880 }
  [Console]::Beep(1200, 300)
  Write-Host "capturing..."
  if ($Vendor -eq "nvenc") {
    # Desktop duplication -> CUDA scale on the GPU -> lossless NVENC (no copies through the CPU).
    $cap = @("-f", "lavfi", "-i", "ddagrab=framerate=60:draw_mouse=0", "-t", "$Seconds", "-vf", "hwmap=derive_device=cuda,scale_cuda=1920:1080:format=yuv420p", "-c:v", "h264_nvenc", "-tune", "lossless", "-preset", "p1", $ref)
  } else {
    $cap = @("-f", "lavfi", "-i", "ddagrab=framerate=60:draw_mouse=0", "-t", "$Seconds", "-vf", "hwdownload,format=bgra,scale=1920:1080:flags=bicubic:out_color_matrix=bt709,format=yuv420p", "-c:v", "libx264", "-preset", "ultrafast", "-qp", "0", $ref)
  }
  Invoke-FF $cap | Out-Null
  [Console]::Beep(1200, 300); [Console]::Beep(900, 300)
  Write-Host "Capture done. Measuring (several minutes; you can stop playing)." -ForegroundColor Cyan
}
$m = ((Invoke-FF @("-i", $ref, "-map", "0:v:0", "-c", "copy", "-f", "null", "-") -Quiet).log | Select-String -Pattern "time=(\d+):(\d+):([\d.]+)" -AllMatches).Matches[-1]
$dur = [int]$m.Groups[1].Value * 3600 + [int]$m.Groups[2].Value * 60 + [double]::Parse($m.Groups[3].Value, [Globalization.CultureInfo]::InvariantCulture)

# ---------- candidates ----------
# Common: keyframe every second (60 frames), like the recorder; no B-frames unless said.
function Args-For([string]$Codec, [string]$Mode, [double]$Value, [switch]$BFrames) {
  $e = $enc[$Codec]
  $a = @("-c:v", $e, "-g", "60")
  switch -Wildcard ($e) {
    "*_nvenc" {
      $a += @("-preset", "p4", "-forced-idr", "1")
      if ($BFrames) { $a += @("-tune", "hq", "-bf", "3") } else { $a += @("-tune", "ll", "-bf", "0") }
      if ($Mode -eq "vbr") { $a += @("-rc", "vbr", "-b:v", "$($Value)M", "-maxrate", "$($Value * 2)M") }
      else { $a += @("-rc", "vbr", "-cq", "$Value", "-b:v", "0", "-maxrate", "60M") }
    }
    "*_amf" {
      if ($BFrames) { $a += @("-usage", "transcoding", "-bf", "3") } else { $a += @("-usage", "lowlatency", "-bf", "0") }
      if ($Mode -eq "vbr") { $a += @("-rc", "vbr_peak", "-b:v", "$($Value)M", "-maxrate", "$($Value * 2)M") }
      else { $a += @("-rc", "qvbr", "-qvbr_quality_level", "$Value", "-b:v", "20M", "-maxrate", "60M") }
    }
    "*_qsv" {
      if ($BFrames) { $a += @("-bf", "3") } else { $a += @("-bf", "0", "-low_power", "1") }
      if ($Mode -eq "vbr") { $a += @("-b:v", "$($Value)M", "-maxrate", "$($Value * 2)M") }
      else { $a += @("-global_quality", "$Value") }
    }
    "libx264" {
      $a += @("-preset", "veryfast")
      if (-not $BFrames) { $a += @("-tune", "zerolatency", "-bf", "0") }
      if ($Mode -eq "vbr") { $a += @("-b:v", "$($Value)M") } else { $a += @("-crf", "$Value") }
    }
    "libx265" {
      $a += @("-preset", "veryfast")
      if (-not $BFrames) { $a += @("-tune", "zerolatency") }
      if ($Mode -eq "vbr") { $a += @("-b:v", "$($Value)M") } else { $a += @("-crf", "$Value") }
    }
    "libsvtav1" {
      $a += @("-preset", "10", "-svtav1-params", "keyint=60:scd=0:pred-struct=1")
      if ($Mode -eq "vbr") { $a += @("-b:v", "$($Value)M") } else { $a += @("-crf", "$Value") }
    }
  }
  return ,$a
}
# Quality values per encoder family: (H.264, HEVC, AV1) ladders.
$isAmf = $Vendor -eq "amf"; $isQsv = $Vendor -eq "qsv"; $isSw = $Vendor -eq "software"
$cqH264 = if ($isAmf) { @(28, 24, 20, 16) } elseif ($isSw) { @(17, 20, 23, 26) } else { @(18, 21, 24, 27) }
$cqHevc = if ($isAmf) { @(28, 24, 20, 16) } elseif ($isSw) { @(19, 22, 25, 28) } else { @(21, 24, 27, 30) }
$cqAv1 = if ($isAmf) { @(28, 24, 20, 16) } elseif ($isSw) { @(25, 30, 35, 40) } elseif ($isQsv) { @(70, 90, 110, 130) } else { @(24, 28, 32, 36) }
$cands = @()
$cands += @{ name = "H.264 VBR 12 Mbps (current Standard)"; codec = "h264"; mode = "vbr"; v = 12; current = $true }
$cands += @{ name = "H.264 VBR 20 Mbps (current High)"; codec = "h264"; mode = "vbr"; v = 20 }
$cands += @{ name = "H.264 VBR 8 Mbps"; codec = "h264"; mode = "vbr"; v = 8 }
foreach ($q in $cqH264) { $cands += @{ name = "H.264 quality $q"; codec = "h264"; mode = "cq"; v = $q } }
$cands += @{ name = "H.264 VBR 12 Mbps + B-frames"; codec = "h264"; mode = "vbr"; v = 12; bf = $true }
if ($enc.hevc) {
  foreach ($b in @(4, 6, 8, 10, 12)) { $cands += @{ name = "HEVC VBR $b Mbps"; codec = "hevc"; mode = "vbr"; v = $b } }
  foreach ($q in $cqHevc) { $cands += @{ name = "HEVC quality $q"; codec = "hevc"; mode = "cq"; v = $q } }
  $cands += @{ name = "HEVC VBR 8 Mbps + B-frames"; codec = "hevc"; mode = "vbr"; v = 8; bf = $true }
}
if ($enc.av1) {
  foreach ($b in @(3, 4.5, 6, 8, 10)) { $cands += @{ name = "AV1 VBR $b Mbps"; codec = "av1"; mode = "vbr"; v = $b } }
  foreach ($q in $cqAv1) { $cands += @{ name = "AV1 quality $q"; codec = "av1"; mode = "cq"; v = $q } }
}

$threads = [Math]::Max(2, [Environment]::ProcessorCount - 2)
function Measure-One([string]$Dist) {
  $r = @{}
  if ($hasVmaf) {
    $log = Join-Path $Out "vmaf.json"
    $lp = $log -replace "\\", "/" -replace ":", "\:"
    Invoke-FF @("-i", $Dist, "-i", $ref, "-lavfi", "[0:v]setpts=PTS-STARTPTS[d];[1:v]setpts=PTS-STARTPTS[r];[d][r]libvmaf=log_fmt=json:log_path='$lp':n_threads=$($threads):feature=name=float_ssim|name=psnr", "-f", "null", "-") | Out-Null
    $j = Get-Content $log -Raw | ConvertFrom-Json
    $v = @($j.frames | ForEach-Object { [double]$_.metrics.vmaf }) | Sort-Object
    $r.vmaf = [double]$j.pooled_metrics.vmaf.mean
    $r.vmaf_low1 = $v[[Math]::Max(0, [int]($v.Count / 100) - 1)]
    $r.ssim = [double]$j.pooled_metrics.float_ssim.mean
    $r.psnr = [double]$j.pooled_metrics.psnr_y.mean
  } else {
    $st = Join-Path $Out "ssim.log"
    $sp = $st -replace "\\", "/" -replace ":", "\:"
    $l = (Invoke-FF @("-i", $Dist, "-i", $ref, "-lavfi", "[0:v]setpts=PTS-STARTPTS,split[a][b];[1:v]setpts=PTS-STARTPTS,split[c][d];[a][c]ssim=stats_file='$sp';[b][d]psnr", "-f", "null", "-")).log
    $vals = @(Get-Content $st | ForEach-Object { if ($_ -match "All:([\d.]+)") { [double]::Parse($Matches[1], [Globalization.CultureInfo]::InvariantCulture) } }) | Sort-Object
    $r.ssim = ($vals | Measure-Object -Average).Average
    $r.ssim_low1 = $vals[[Math]::Max(0, [int]($vals.Count / 100) - 1)]
    if ($l -match "PSNR .*average:([\d.]+)") { $r.psnr = [double]::Parse($Matches[1], [Globalization.CultureInfo]::InvariantCulture) }
  }
  return $r
}

$results = @()
foreach ($c in $cands) {
  $file = Join-Path $Out (($c.name -replace "[^A-Za-z0-9]+", "_").Trim("_") + ".mp4")
  $a = Args-For $c.codec $c.mode $c.v -BFrames:([bool]$c.bf)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $res = Invoke-FF (@("-i", $ref, "-an") + $a + @($file)) -Quiet
  $encSecs = $sw.Elapsed.TotalSeconds
  if (-not $res.ok) { Write-Host "  $($c.name): encoder refused these settings, skipped" -ForegroundColor Yellow; continue }
  $size = (Get-Item $file).Length
  $q = Measure-One $file
  $row = [ordered]@{ name = $c.name; codec = $c.codec; mode = $c.mode; value = $c.v; bframes = [bool]$c.bf; current = [bool]$c.current; mbps = [Math]::Round($size * 8 / $dur / 1e6, 2); mb_per_min = [Math]::Round($size / $dur * 60 / 1e6, 1); encode_fps = [Math]::Round(60 * $dur / [Math]::Max($encSecs, 0.01), 0) }
  foreach ($k in $q.Keys) { $row[$k] = [Math]::Round([double]$q[$k], 4) }
  $results += [pscustomobject]$row
  $qual = if ($hasVmaf) { "VMAF {0,6:N2}  1% low {1,6:N2}  SSIM {2:N4}" -f $row.vmaf, $row.vmaf_low1, $row.ssim } else { "SSIM {0:N4}  1% low {1:N4}" -f $row.ssim, $row.ssim_low1 }
  Write-Host ("  {0,-40} {1,6:N2} Mbps  {2,6:N1} MB/min  {3}  PSNR {4:N2}" -f $c.name, $row.mbps, $row.mb_per_min, $qual, $row.psnr)
}

# ---------- verdict: the smallest file with the same quality or better ----------
$cur = $results | Where-Object { $_.current } | Select-Object -First 1
$key = if ($hasVmaf) { "vmaf" } else { "ssim" }
$low = if ($hasVmaf) { "vmaf_low1" } else { "ssim_low1" }
$tolMean = if ($hasVmaf) { 0.5 } else { 0.0005 }
$tolLow = if ($hasVmaf) { 1.0 } else { 0.002 }
$lines = @()
$lines += "Encoder comparison $(Get-Date -Format 'yyyy-MM-dd HH:mm'), $Vendor, reference $([Math]::Round($dur, 1)) s, metric $key"
$lines += ""
foreach ($r in $results) {
  $lines += ("{0,-40} {1,6:N2} Mbps {2,6:N1} MB/min  {3}={4:N4}  {5}={6:N4}  PSNR={7:N2}  {8} fps" -f $r.name, $r.mbps, $r.mb_per_min, $key, $r.$key, $low, $r.$low, $r.psnr, $r.encode_fps)
}
$lines += ""
$lines += "Same quality or better than the current setting ($key mean >= $([Math]::Round($cur.$key - $tolMean, 4)), worst 1 % >= $([Math]::Round($cur.$low - $tolLow, 4))), smallest first:"
$ok = $results | Where-Object { -not $_.current -and $_.$key -ge $cur.$key - $tolMean -and $_.$low -ge $cur.$low - $tolLow } | Sort-Object mbps
foreach ($r in $ok) { $lines += ("  {0,-40} {1,6:N2} Mbps  ({2:N0} % of the current size)" -f $r.name, $r.mbps, (100 * $r.mbps / $cur.mbps)) }
if (-not $ok) { $lines += "  none" }
$lines | Set-Content -Path (Join-Path $Out "results.txt") -Encoding UTF8
$results | ConvertTo-Json -Depth 4 | Set-Content -Path (Join-Path $Out "results.json") -Encoding UTF8
Write-Host ""
$lines | Select-Object -Last ($ok.Count + 2) | ForEach-Object { Write-Host $_ }
Write-Host ""
Write-Host "Saved: $(Join-Path $Out 'results.txt') (send it to Claude)." -ForegroundColor Green
