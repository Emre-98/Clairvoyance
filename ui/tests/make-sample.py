#!/usr/bin/env python3
"""Makes the UI tests' sample videos (ui/public/dev-assets/, not committed) and placeholder
thumbnails.

The video is 4:3 (640x480, 30 fps, 160 s) with a 16:9 "game" letterboxed inside it (like the
recorder does when the game's aspect differs from the output). Each frame shows the "cursor" as a
magenta box at the position the synthetic input data (lib/inputoverlay.ts `synthetic`) has at
that frame's own timestamp: (0.5 + 0.3 cos t, 0.5 + 0.3 sin t) of the game area. WebM stores
timestamps in whole milliseconds, so frame k starts at round(k * 1000 / 30) ms (the same rule as
`sampleFrameOf` in lib/bubbles.ts).

sample169.webm / sample169old.webm (v1.6 player tests): 16:9, 960x540, 60 fps, 160 s, the whole
frame is the "game" (no letterbox), the magenta cursor box as above, and each frame's index
written as 14 white/black squares in the top-left corner (read back by the frame-step tests).
Keyframes every second (like new recordings) / every 5.5 s (like the oldest ones). Next to each:
<name>.frames.json (every frame's start time, from ffprobe) and <name>.keyframes.json.

    python3 tests/make-sample.py [--only169]      (needs numpy and ffmpeg with libvpx-vp9)
"""
import math, os, subprocess, sys
import numpy as np

W, H, FPS, DUR = 640, 480, 30, 160
CW, CH = W, W * 9 // 16          # game area 640x360
CY = (H - CH) // 2                # letterbox bars of 60 px
BOX = 8
out_dir = os.path.join(os.path.dirname(__file__), "..", "public", "dev-assets")
os.makedirs(out_dir, exist_ok=True)
out = os.path.join(out_dir, "sample.webm")

yy, xx = np.mgrid[0:CH, 0:CW]
game = np.zeros((CH, CW, 3), np.uint8)
game[..., 0] = 40 + (xx * 30 // CW)
game[..., 1] = 60 + (yy * 40 // CH)
game[..., 2] = 52
base = np.zeros((H, W, 3), np.uint8)
base[CY:CY + CH] = game

p = subprocess.Popen(
    ["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-",
     "-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-b:v", "600k", "-g", "30", "-pix_fmt", "yuv420p", out],
    stdin=subprocess.PIPE)
only169 = "--only169" in sys.argv
for k in range(0 if only169 else DUR * FPS):
    t = round(k * 1000 / FPS) / 1000
    x = (0.5 + 0.3 * math.cos(t)) * CW
    y = (0.5 + 0.3 * math.sin(t)) * CH + CY
    f = base.copy()
    x0, y0 = int(round(x - BOX / 2)), int(round(y - BOX / 2))
    f[y0:y0 + BOX, x0:x0 + BOX] = (255, 0, 255)
    p.stdin.write(f.tobytes())
p.stdin.close()
if p.wait():
    sys.exit("ffmpeg failed")


BITS, BS, BX, BY = 14, 20, 6, 6   # frame index: 14 squares of 20 px, 6 px apart


def make169(name, gop, w=960, h=540, fps=60, dur=160):
    path = os.path.join(out_dir, name + ".webm")
    yy, xx = np.mgrid[0:h, 0:w]
    bg = np.zeros((h, w, 3), np.uint8)
    bg[..., 0] = 40 + (xx * 60 // w)
    bg[..., 1] = 70 + (yy * 60 // h)
    bg[..., 2] = 90
    q = subprocess.Popen(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{w}x{h}", "-r", str(fps), "-i", "-",
         "-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-b:v", "1500k", "-g", str(gop), "-keyint_min", str(gop),
         "-pix_fmt", "yuv420p", path],
        stdin=subprocess.PIPE)
    for k in range(dur * fps):
        t = round(k * 1000 / fps) / 1000
        f = bg.copy()
        for b in range(BITS):
            x0 = BX + b * (BS + 6)
            f[BY:BY + BS, x0:x0 + BS] = 255 if (k >> b) & 1 else 0
        x = (0.5 + 0.3 * math.cos(t)) * w
        y = (0.5 + 0.3 * math.sin(t)) * h
        x0, y0 = int(round(x - 6)), int(round(y - 6))
        f[y0:y0 + 12, x0:x0 + 12] = (255, 0, 255)
        q.stdin.write(f.tobytes())
    q.stdin.close()
    if q.wait():
        sys.exit("ffmpeg failed")
    import json
    out = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "frame=pts_time,key_frame",
                          "-of", "csv=p=0", path], capture_output=True, text=True, check=True).stdout.split()
    frames, keys = [], []
    for line in out:
        kf, pt = line.split(",")[:2]
        frames.append(float(pt))
        if kf == "1":
            keys.append(float(pt))
    with open(os.path.join(out_dir, name + ".frames.json"), "w") as fh:
        json.dump(frames, fh)
    with open(os.path.join(out_dir, name + ".keyframes.json"), "w") as fh:
        json.dump(keys, fh)
    print("wrote", path, len(frames), "frames,", len(keys), "keyframes")


make169("sample169", 60)
make169("sample169old", 330)
for i in range(12):
    hue = i * 30
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "lavfi", "-i", f"color=c=0x{(hue * 5) % 255:02x}3050:s=480x270",
                    "-frames:v", "1", os.path.join(out_dir, f"thumb{i}.jpg")], check=True)
print("wrote", out)
