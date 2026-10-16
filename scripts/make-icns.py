#!/usr/bin/env python3
"""Builds assets/PostRonda.icns from the master icon (assets/icon.webp).

`iconutil` re-encodes every layer and ends up with a file about 2.5x larger than the PNGs it
was given, so the container is written here directly with losslessly compressed PNGs.
Requires ImageMagick (`magick`) to scale the master.
"""
import pathlib, struct, subprocess, sys, tempfile

root = pathlib.Path(__file__).resolve().parent.parent
master = root / "assets" / "icon.webp"
out = root / "assets" / "PostRonda.icns"
# icns type -> pixel size (PNG payloads; the @2x variants reuse the larger bitmap).
layers = [
    ("icp4", 16), ("icp5", 32), ("ic11", 32), ("ic12", 64),
    ("ic07", 128), ("ic13", 256), ("ic08", 256), ("ic14", 512),
    ("ic09", 512), ("ic10", 1024),
]
with tempfile.TemporaryDirectory() as tmp:
    pngs = {}
    for px in sorted({size for _, size in layers}):
        path = pathlib.Path(tmp) / f"{px}.png"
        subprocess.run(
            ["magick", str(master), "-resize", f"{px}x{px}",
             "-define", "png:compression-level=9", "-define", "png:exclude-chunks=all",
             f"PNG32:{path}"],
            check=True,
        )
        pngs[px] = path.read_bytes()
chunks = b"".join(
    kind.encode() + struct.pack(">I", 8 + len(pngs[size])) + pngs[size] for kind, size in layers
)
out.write_bytes(b"icns" + struct.pack(">I", 8 + len(chunks)) + chunks)
print(f"wrote {out} ({out.stat().st_size / 1024:.0f} KB)", file=sys.stderr)
