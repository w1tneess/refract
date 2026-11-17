"""Regenerate the synthetic golden fixtures.

Needs: pip install pillow-heif==1.8.1 (bundles libheif 1.23.6 + x265).
Run from the repo root: python fixtures/make_fixtures.py

iphone.heic is NOT generated here: it must be a real iPhone photo (see fixtures/README.md).
"""
import io
from pathlib import Path

from PIL import Image
import pillow_heif

pillow_heif.register_heif_opener()
OUT = Path(__file__).parent


def marker_image(w: int, h: int) -> Image.Image:
    """Blue image with a red block in the stored top-left corner, so a wrong rotation shows."""
    img = Image.new("RGB", (w, h), (0, 0, 255))
    img.paste((255, 0, 0), (0, 0, w // 4, h // 4))
    return img


def heic_bytes(img: Image.Image, **kw) -> bytes:
    buf = io.BytesIO()
    img.save(buf, format="HEIF", quality=80, **kw)
    return buf.getvalue()


# Plain landscape HEIC, 400x300.
plain = heic_bytes(marker_image(400, 300))
(OUT / "plain.heic").write_bytes(plain)

# Rotated: stored 400x300, EXIF orientation 6 (90 CW). pillow-heif turns it into an irot box,
# so the displayed image is 300x400 with the red block at the top-right.
exif = Image.Exif()
exif[0x0112] = 6
(OUT / "rotated.heic").write_bytes(heic_bytes(marker_image(400, 300), exif=exif.tobytes()))

# A HEIC saved with a .jpg extension.
(OUT / "really-heic.jpg").write_bytes(plain)

# Bengali + emoji filename.
(OUT / "ছবি 😀.heic").write_bytes(plain)

# 0-byte file.
(OUT / "empty.heic").write_bytes(b"")

# Truncated: cut inside mdat (after meta), so Probe passes and decoding fails -> Failed/Corrupt.
mdat = plain.find(b"mdat")
assert mdat > 0
(OUT / "truncated.heic").write_bytes(plain[: mdat + 4 + (len(plain) - mdat) // 3])

# Large HEIC (24 MP) so the cancel test has time to click Cancel mid-job.
# A Mandelbrot render keeps the file small while decode time still scales with pixels.
big = Image.effect_mandelbrot((6000, 4000), (-2.0, -1.5, 1.0, 1.5), 100).convert("RGB")
(OUT / "large.heic").write_bytes(heic_bytes(big))

# Plain landscape PNG with alpha, 400x300: blue with opaque top-left red block
png_img = Image.new("RGBA", (400, 300), (0, 0, 255, 255))
png_img.paste((255, 0, 0, 255), (0, 0, 100, 75))
buf_png = io.BytesIO()
png_img.save(buf_png, format="PNG")
(OUT / "plain.png").write_bytes(buf_png.getvalue())

# Plain WebP, 400x300
buf_webp = io.BytesIO()
png_img.save(buf_webp, format="WEBP", quality=80)
(OUT / "plain.webp").write_bytes(buf_webp.getvalue())

# Plain JPEG, 400x300
buf_jpg = io.BytesIO()
marker_image(400, 300).save(buf_jpg, format="JPEG", quality=80)
plain_jpg = buf_jpg.getvalue()
(OUT / "plain.jpg").write_bytes(plain_jpg)

# Rotated JPEG with EXIF 6: stored 400x300, displays 300x400
exif_jpg = Image.Exif()
exif_jpg[0x0112] = 6
buf_rot_jpg = io.BytesIO()
marker_image(400, 300).save(buf_rot_jpg, format="JPEG", quality=80, exif=exif_jpg)
(OUT / "rotated.jpg").write_bytes(buf_rot_jpg.getvalue())

# A PNG saved with .jpg extension
(OUT / "really-png.jpg").write_bytes(buf_png.getvalue())

# Truncated JPEG
(OUT / "truncated.jpg").write_bytes(plain_jpg[:len(plain_jpg) // 2])

for p in sorted(OUT.iterdir()):
    print(f"{p.name}: {p.stat().st_size} bytes")
