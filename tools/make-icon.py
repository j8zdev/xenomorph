#!/usr/bin/env python3
"""Renders assets/icon.svg to icon-256.png, icon-512.png and icon.ico (needs resvg)."""
import os
import struct
import subprocess
import tempfile

assets = os.path.join(os.path.dirname(__file__), "..", "assets")
svg = os.path.join(assets, "icon.svg")


def render(size, target):
    subprocess.run(["resvg", "-w", str(size), "-h", str(size), svg, target], check=True)


render(256, os.path.join(assets, "icon-256.png"))
render(512, os.path.join(assets, "icon-512.png"))

# An .ico file is a small directory followed by the PNG images.
sizes = [16, 32, 48, 64, 256]
images = []
with tempfile.TemporaryDirectory() as tmp:
    for size in sizes:
        path = os.path.join(tmp, f"{size}.png")
        render(size, path)
        with open(path, "rb") as f:
            images.append(f.read())

out = struct.pack("<HHH", 0, 1, len(sizes))
offset = 6 + 16 * len(sizes)
for size, data in zip(sizes, images):
    out += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
with open(os.path.join(assets, "icon.ico"), "wb") as f:
    f.write(out + b"".join(images))
