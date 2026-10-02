#!/usr/bin/env python3
"""Makes the UI tests' sample video (ui/public/dev-assets/sample.webm, not committed) and
placeholder thumbnails.

The video is 4:3 (640x480, 30 fps, 160 s) with a 16:9 "game" letterboxed inside it (like the
recorder does when the game's aspect differs from the output). Each frame shows the "cursor" as a
magenta box at the position the synthetic input data (lib/inputoverlay.ts `synthetic`) has at
that frame's own timestamp: (0.5 + 0.3 cos t, 0.5 + 0.3 sin t) of the game area. WebM stores
timestamps in whole milliseconds, so frame k starts at round(k * 1000 / 30) ms (the same rule as
`sampleFrameOf` in lib/bubbles.ts).

    python3 tests/make-sample.py      (needs numpy and ffmpeg with libvpx-vp9)
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
for k in range(DUR * FPS):
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
for i in range(12):
    hue = i * 30
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "lavfi", "-i", f"color=c=0x{(hue * 5) % 255:02x}3050:s=480x270",
                    "-frames:v", "1", os.path.join(out_dir, f"thumb{i}.jpg")], check=True)
print("wrote", out)
