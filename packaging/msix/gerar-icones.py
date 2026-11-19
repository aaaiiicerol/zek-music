#!/usr/bin/env python3
"""Gera os PNGs do MSIX a partir de packaging/zek-music.png.
Uso:  pip install pillow   &&   python3 packaging/msix/gerar-icones.py
"""
import os
from PIL import Image, ImageDraw

here = os.path.dirname(os.path.abspath(__file__))
src = Image.open(os.path.join(here, "..", "zek-music.png")).convert("RGBA")
assets = os.path.join(here, "Assets")
art = os.path.join(here, "store-art")
os.makedirs(assets, exist_ok=True)
os.makedirs(art, exist_ok=True)

def square(n):
    return src.resize((n, n), Image.LANCZOS)

def gradient(w, h):
    top, bottom = (30, 79, 216), (5, 10, 28)
    img = Image.new("RGB", (w, h))
    d = ImageDraw.Draw(img)
    for y in range(h):
        t = y / max(h - 1, 1)
        d.line([(0, y), (w, y)], fill=tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(3)))
    return img.convert("RGBA")

def on_gradient(w, h, icon):
    bg = gradient(w, h)
    bg.alpha_composite(square(icon), ((w - icon) // 2, (h - icon) // 2))
    return bg

square(44).save(os.path.join(assets, "Square44x44Logo.png"))
square(150).save(os.path.join(assets, "Square150x150Logo.png"))
square(50).save(os.path.join(assets, "StoreLogo.png"))                      # exigido pelo manifest
on_gradient(310, 150, 118).save(os.path.join(assets, "Wide310x150Logo.png"))
square(300).save(os.path.join(art, "Square300x300.png"))                    # imagem da página da Store
on_gradient(720, 1080, 460).save(os.path.join(art, "Poster720x1080.png"))  # capa da página da Store
print("PNGs gerados em", assets, "e", art)
