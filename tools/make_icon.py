#!/usr/bin/env python3
"""Generate the Halftone icon: a halftone gradient orb — dot grid whose dot
sizes grow along a diagonal (the halftone shading idea), with a play-triangle
dithered into the negative space. Fully deterministic, drawn at 1024px then
downscaled to each ICO/PNG size."""
from PIL import Image, ImageDraw
import math, os

OUT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons")
os.makedirs(OUT, exist_ok=True)

S = 1024          # master canvas
BG = (13, 15, 17, 255)       # near-black, matches app bg #0D0F11
ACC = (102, 224, 194, 255)   # halftone mint #66E0C2
ACC_DIM = (56, 120, 106, 255)

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

# rounded-square dark field (the "widget")
def rounded(draw, box, r, fill):
    draw.rounded_rectangle(box, radius=r, fill=fill)

margin = 24
rounded(d, (margin, margin, S-margin, S-margin), 200, BG)

# halftone dot grid: dot radius grows along the diagonal (light top-left -> dense bottom-right)
# and a circular mask keeps the orb silhouette
cx, cy, R = S/2, S/2, 400
grid = 26          # cell size
for gy in range(margin, S-margin, grid):
    for gx in range(margin, S-margin, grid):
        mx, my = gx+grid/2, gy+grid/2
        # orb mask
        if math.hypot(mx-cx, my-cy) > R: continue
        # diagonal growth 0..1 + slight radial falloff for depth
        t = ((mx-margin)/(S-2*margin) + (my-margin)/(S-2*margin)) / 2
        t = max(0.0, min(1.0, t))
        rmax = grid*0.48
        r = 1.4 + t*t*2.2*rmax          # quadratic growth = classic scan gradient
        # color ramp: dim in light areas, full accent in dense areas
        col = tuple(int(ACC_DIM[i] + (ACC[i]-ACC_DIM[i]) * t) for i in range(3)) + (255,)
        d.ellipse((mx-r, my-r, mx+r, my+r), fill=col)

# carve a play triangle out of the dot field (negative space: fine bg-colored dot dither)
tri = [(cx-105, cy-150), (cx-105, cy+150), (cx+190, cy)]
def in_tri(px, py, t):
    def cross(o, a, b):
        return (a[0]-o[0])*(py-o[1]) - (a[1]-o[1])*(px-o[0])
    c1 = cross(t[0], t[1], (px,py)) if False else (
        (t[1][0]-t[0][0])*(py-t[0][1]) - (t[1][1]-t[0][1])*(px-t[0][0]))
    c2 = (t[2][0]-t[1][0])*(py-t[1][1]) - (t[2][1]-t[1][1])*(px-t[1][0])
    c3 = (t[0][0]-t[2][0])*(py-t[2][1]) - (t[0][1]-t[2][1])*(px-t[2][0])
    return (c1>=0)==(c2>=0)==(c3>=0)
for gy in range(margin, S-margin, 13):
    for gx in range(margin, S-margin, 13):
        mx, my = gx+6, gy+6
        if in_tri(mx, my, tri):
            r = 4.2
            d.ellipse((mx-r, my-r, mx+r, my+r), fill=BG)

img.save(os.path.join(OUT, "icon_master.png"))

# sizes for .ico + pngs
ico_sizes = [16, 24, 32, 48, 64, 128, 256]
png_sizes = [128, 256, 512, 1024]
imgs = {sz: img.resize((sz, sz), Image.LANCZOS) for sz in set(ico_sizes+png_sizes)}
img.resize((256,256), Image.LANCZOS).save(os.path.join(OUT, "icon.ico"), sizes=[(s,s) for s in ico_sizes])
for sz in png_sizes:
    imgs[sz].save(os.path.join(OUT, f"{sz}x{sz}.png"))
# tauri conventional names
imgs[128].save(os.path.join(OUT, "128x128.png"))
imgs[256].save(os.path.join(OUT, "128x128@2x.png"))
imgs[512].save(os.path.join(OUT, "icon.png"))
print("icons written to", os.path.abspath(OUT))
