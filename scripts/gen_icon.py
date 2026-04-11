#!/usr/bin/env python3
# /// script
# requires-python = ">=3.9"
# dependencies = ["cairosvg", "Pillow"]
# ///
"""Generate data/razer.ico from data/razer-blade-control.svg.

Usage:
    uv run scripts/gen_icon.py
"""
import io
from pathlib import Path

import cairosvg
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SVG  = ROOT / "data" / "razer-blade-control.svg"
ICO  = ROOT / "data" / "razer.ico"

SIZES = [16, 32, 48, 256]

# Render at 256 (largest); Pillow will auto-downscale for all smaller ICO frames.
png = cairosvg.svg2png(url=str(SVG), output_width=256, output_height=256)
img = Image.open(io.BytesIO(png)).convert("RGBA")

img.save(
    ICO,
    format="ICO",
    sizes=[(s, s) for s in SIZES],
)
print(f"✓  {ICO}")
