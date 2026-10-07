#!/usr/bin/env python3
"""Encoder comparison on Claude's Linux machine (no GPU): software encoders standing in for the
hardware ones, set up like the recorder (keyframe every second, low latency, no B-frames unless
said), against a near-lossless reference. Reports size, SSIM (mean, the fight at 15-22 s, the
worst 1 % of frames) and PSNR. The real decision comes from `encoder-compare.ps1` on the
owner's PC (NVENC / AMF / Quick Sync + VMAF); this shows the trends.

    python3 compare_sw.py <ref.mkv> <workdir>      -> <workdir>/results.json + a table
"""
import json
import math
import os
import re
import subprocess
import sys

REF, WD = sys.argv[1], sys.argv[2]
os.makedirs(WD, exist_ok=True)
G = ["-g", "60", "-keyint_min", "60"]
X264_LL = ["-c:v", "libx264", "-preset", "veryfast", "-tune", "zerolatency", "-bf", "0", *G]
X265_LL = ["-c:v", "libx265", "-preset", "veryfast", "-tune", "zerolatency", *G]
SVT = lambda extra: ["-c:v", "libsvtav1", "-preset", "10", "-g", "60", "-svtav1-params", f"keyint=60:scd=0{extra}"]

CANDIDATES = [
    # (name, group, args)
    ("h264 vbr 12M (current)", "h264-bitrate", [*X264_LL, "-b:v", "12M"]),
    ("h264 vbr 8M", "h264-bitrate", [*X264_LL, "-b:v", "8M"]),
    ("h264 vbr 16M", "h264-bitrate", [*X264_LL, "-b:v", "16M"]),
    ("h264 vbr 20M (High)", "h264-bitrate", [*X264_LL, "-b:v", "20M"]),
    ("h264 crf 20", "h264-quality", [*X264_LL, "-crf", "20"]),
    ("h264 crf 23", "h264-quality", [*X264_LL, "-crf", "23"]),
    ("h264 crf 26", "h264-quality", [*X264_LL, "-crf", "26"]),
    ("h264 vbr 12M + 3 B-frames", "h264-bframes", ["-c:v", "libx264", "-preset", "veryfast", "-bf", "3", *G, "-b:v", "12M"]),
    ("hevc vbr 4M", "hevc-bitrate", [*X265_LL, "-b:v", "4M"]),
    ("hevc vbr 6M", "hevc-bitrate", [*X265_LL, "-b:v", "6M"]),
    ("hevc vbr 8M", "hevc-bitrate", [*X265_LL, "-b:v", "8M"]),
    ("hevc vbr 12M", "hevc-bitrate", [*X265_LL, "-b:v", "12M"]),
    ("hevc crf 24", "hevc-quality", [*X265_LL, "-crf", "24"]),
    ("hevc crf 27", "hevc-quality", [*X265_LL, "-crf", "27"]),
    ("hevc vbr 8M + B-frames", "hevc-bframes", ["-c:v", "libx265", "-preset", "veryfast", *G, "-b:v", "8M"]),
    ("av1 vbr 3M", "av1-bitrate", [*SVT(":pred-struct=1"), "-b:v", "3M"]),
    ("av1 vbr 4.5M", "av1-bitrate", [*SVT(":pred-struct=1"), "-b:v", "4500k"]),
    ("av1 vbr 6M", "av1-bitrate", [*SVT(":pred-struct=1"), "-b:v", "6M"]),
    ("av1 vbr 9M", "av1-bitrate", [*SVT(":pred-struct=1"), "-b:v", "9M"]),
    ("av1 crf 32", "av1-quality", [*SVT(":pred-struct=1"), "-crf", "32"]),
    ("av1 crf 38", "av1-quality", [*SVT(":pred-struct=1"), "-crf", "38"]),
]


def run(args):
    return subprocess.run(["ffmpeg", "-hide_banner", "-nostdin", "-y", *args], capture_output=True, text=True)


def metrics(path):
    stats = os.path.join(WD, "ssim.log")
    r = run(["-i", path, "-i", REF, "-lavfi", f"[0:v]split[a][b];[1:v]split[c][d];[a][c]ssim=stats_file={stats};[b][d]psnr", "-f", "null", "-"])
    psnr = float(re.search(r"PSNR .*average:([\d.inf]+)", r.stderr).group(1))
    per = []
    for line in open(stats):
        m = re.search(r"n:(\d+).* All:([\d.]+)", line)
        if m:
            per.append((int(m.group(1)), float(m.group(2))))
    vals = sorted(v for _, v in per)
    fight = [v for n, v in per if 15 * 60 <= n < 22 * 60]
    return {
        "ssim": sum(vals) / len(vals),
        "ssim_fight": sum(fight) / len(fight),
        "ssim_low1": vals[max(0, len(vals) // 100 - 1)],
        "psnr": psnr,
    }


db = lambda s: -10 * math.log10(max(1e-9, 1 - s))
out = []
dur = 30.0
for name, group, args in CANDIDATES:
    ext = "mp4"
    path = os.path.join(WD, re.sub(r"[^a-z0-9]+", "_", name.lower()) + "." + ext)
    r = run(["-i", REF, *args, "-an", path])
    if r.returncode != 0:
        print("FAILED", name, r.stderr[-400:])
        continue
    size = os.path.getsize(path)
    m = metrics(path)
    row = {"name": name, "group": group, "mbps": size * 8 / dur / 1e6, "mb_per_min": size / dur * 60 / 1e6, **m}
    out.append(row)
    print(f"{name:28s} {row['mbps']:6.2f} Mbps  SSIM {m['ssim']:.4f} ({db(m['ssim']):.2f} dB)  fight {m['ssim_fight']:.4f}  low1% {m['ssim_low1']:.4f}  PSNR {m['psnr']:.2f}", flush=True)
json.dump(out, open(os.path.join(WD, "results.json"), "w"), indent=1)
