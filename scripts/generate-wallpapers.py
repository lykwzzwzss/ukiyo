"""Export launcher wallpapers directly from their source images at JPEG quality 95."""
import argparse
from pathlib import Path

from PIL import Image, ImageOps

parser = argparse.ArgumentParser()
parser.add_argument("source", type=Path)
args = parser.parse_args()
sources = sorted(p for p in args.source.iterdir() if p.suffix.lower() in {".jpg", ".jpeg", ".jfif", ".png", ".webp"})
if not sources:
    parser.error("No supported source images found")
names = [source.stem.casefold() for source in sources]
if len(set(names)) != len(names):
    parser.error("Source images have duplicate base names; rename them before JPEG conversion")
output = Path(__file__).resolve().parents[1] / "src-tauri" / "assets" / "wallpapers"
output.mkdir(parents=True, exist_ok=True)
for source in sources:
    destination = output / f"{source.stem}.jpg"
    with Image.open(source) as original:
        image = ImageOps.exif_transpose(original).convert("RGB")
        options = {"quality": 95, "subsampling": 0, "optimize": True, "progressive": True}
        if original.info.get("icc_profile"):
            options["icc_profile"] = original.info["icc_profile"]
        image.save(destination, "JPEG", **options)
    with Image.open(destination) as result:
        result.verify()
    print(f"{source.name} -> {destination.name}: {destination.stat().st_size} bytes")
