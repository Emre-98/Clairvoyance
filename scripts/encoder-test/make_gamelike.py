#!/usr/bin/env python3
"""A game-like 1080p60 test clip for encoder comparisons when no real recording is at hand
(Claude's Linux machine). It imitates what makes League footage hard or easy to compress:
- a detailed top-down map that pans smoothly, sometimes jumps (minimap click), sometimes rests;
- ~25 units with coloured rims and health bars walking around;
- particle bursts (spells, team fights): many small bright moving dots, the hardest part;
- the real bottom HUD (crops from the owner's recordings, games/league/tests/hud), a minimap,
  a KDA / gold / clock text line and chat lines that change: sharp static detail.

Writes raw rgb24 frames to stdout (pipe into ffmpeg):
    python3 make_gamelike.py [seconds] | ffmpeg -f rawvideo -pix_fmt rgb24 -s 1920x1080 -r 60 -i - ...
Deterministic (fixed seed). It is a stand-in: real games decide the final numbers.
"""
import math
import os
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

W, H, FPS = 1920, 1080, 60
SECS = float(sys.argv[1]) if len(sys.argv) > 1 else 30.0
rng = np.random.default_rng(7)
HUD_DIR = os.path.join(os.path.dirname(__file__), "..", "..", "games", "league", "tests", "hud")


def noise(size, scale, octaves=5):
    out = np.zeros((size, size), np.float32)
    amp = 1.0
    for o in range(octaves):
        n = max(2, size // (scale >> o if scale >> o else 1))
        g = rng.random((n + 1, n + 1)).astype(np.float32)
        img = Image.fromarray((g * 255).astype(np.uint8)).resize((size, size), Image.BICUBIC)
        out += amp * (np.asarray(img, np.float32) / 255.0)
        amp *= 0.5
    return out / out.max()


# The map: grass / dirt / rock / water by height, plus lanes and fine texture.
MS = 3072
h = noise(MS, 512)
fine = noise(MS, 16, 3)
col = np.zeros((MS, MS, 3), np.float32)
grass = np.array([52, 96, 48], np.float32)
dirt = np.array([110, 92, 60], np.float32)
rock = np.array([90, 90, 96], np.float32)
water = np.array([40, 70, 110], np.float32)
t = h[..., None]
col = np.where(t < 0.35, water, np.where(t < 0.62, grass, np.where(t < 0.8, dirt, rock)))
col = col * (0.75 + 0.5 * fine[..., None])
yy, xx = np.mgrid[0:MS, 0:MS]
for k in range(3):  # three lanes
    d = np.abs((xx - yy * (0.4 + 0.3 * k)) - 600 * k) if k != 1 else np.abs(xx + yy - MS)
    lane = d < 70
    col[lane] = col[lane] * 0.4 + np.array([140, 120, 85]) * 0.6
mapimg = np.clip(col, 0, 255).astype(np.uint8)
del col, h, fine, xx, yy, t, d, lane

hud_names = [n for n in sorted(os.listdir(HUD_DIR)) if n.startswith("band-") and "none" not in n]


def ppm(path):
    return Image.open(path).convert("RGB")


huds = [ppm(os.path.join(HUD_DIR, n)) for n in hud_names]
mini = Image.fromarray(mapimg).resize((260, 260), Image.BILINEAR)
font = ImageFont.load_default()

units = [{"x": rng.uniform(800, 2200), "y": rng.uniform(800, 2200), "vx": rng.uniform(-90, 90), "vy": rng.uniform(-90, 90), "c": (220, 60, 60) if i % 2 else (60, 140, 230), "hp": rng.uniform(0.3, 1)} for i in range(25)]
bursts = []

frames = int(SECS * FPS)
cam = np.array([1200.0, 1200.0])
for f in range(frames):
    ts = f / FPS
    # Camera: rests 0-3 s, pans, jumps every ~7 s (minimap click), follows a fight at 15-22 s.
    phase = ts % 7.0
    if phase < 0.02 and ts > 1:
        cam = np.array([rng.uniform(600, 2200), rng.uniform(600, 2200)])
    elif phase > 2.5:
        cam += np.array([math.cos(ts * 0.7) * 6.0, math.sin(ts * 0.5) * 4.0])
    cam = np.clip(cam, 0, MS - max(W, H))
    cx, cy = int(cam[0]), int(cam[1])
    frame = Image.fromarray(mapimg[cy : cy + H, cx : cx + W].copy())
    d = ImageDraw.Draw(frame, "RGBA")
    for u in units:
        u["x"] += u["vx"] / FPS
        u["y"] += u["vy"] / FPS
        if not 600 < u["x"] < 2400:
            u["vx"] *= -1
        if not 600 < u["y"] < 2400:
            u["vy"] *= -1
        sx, sy = u["x"] - cx, u["y"] - cy
        if -60 < sx < W + 60 and -60 < sy < H + 60:
            d.ellipse([sx - 22, sy - 22, sx + 22, sy + 22], fill=(30, 30, 35, 255), outline=u["c"], width=4)
            d.rectangle([sx - 26, sy - 40, sx + 26, sy - 33], fill=(10, 10, 10, 255))
            d.rectangle([sx - 25, sy - 39, sx - 25 + 50 * u["hp"], sy - 34], fill=(70, 200, 70, 255))
    # Spells: a burst every 0.4 s, many during the fight (15-22 s).
    rate = 0.15 if 15 < ts < 22 else 0.025
    if rng.random() < rate:
        bursts.append({"x": rng.uniform(300, W - 300), "y": rng.uniform(200, H - 300), "t0": ts, "p": rng.normal(0, 1, (120, 2)), "c": tuple(int(c) for c in rng.integers(120, 256, 3))})
    bursts = [b for b in bursts if ts - b["t0"] < 1.2]
    for b in bursts:
        age = ts - b["t0"]
        a = int(255 * (1 - age / 1.2))
        for px, py in b["p"]:
            x, y = b["x"] + px * 160 * age, b["y"] + py * 160 * age
            d.ellipse([x - 3, y - 3, x + 3, y + 3], fill=b["c"] + (a,))
    # HUD: the real bottom band (changes every 2 s), minimap, top bar text, chat.
    hud = huds[int(ts / 2) % len(huds)]
    frame.paste(hud, (W // 2 - 180, H - 100))
    frame.paste(mini, (W - 270, H - 270))
    d.rectangle([W - 270 + (cx * 260) // MS, H - 270 + (cy * 260) // MS, W - 270 + ((cx + W) * 260) // MS, H - 270 + ((cy + H) * 260) // MS], outline=(255, 255, 255, 255))
    d.rectangle([W // 2 - 220, 0, W // 2 + 220, 26], fill=(12, 16, 22, 255))
    d.text((W // 2 - 200, 7), f"BLUE {int(ts // 5)}  vs  RED {int(ts // 7)}    {int(ts) // 60:02d}:{int(ts) % 60:02d}    CS {int(ts * 0.8)}", fill=(230, 230, 230, 255), font=font)
    for k in range(4):
        d.text((20, H - 220 + 16 * k), f"[{int(ts) - 4 + k:03d}] Player{k}: line {int(ts * 3) - k}", fill=(240, 220, 140, 255), font=font)
    sys.stdout.buffer.write(frame.tobytes())
