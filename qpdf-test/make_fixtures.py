"""Make locked PDF fixtures with pypdf (independent of qpdf)."""
import io
import json
from pathlib import Path

from pypdf import PdfReader, PdfWriter

OUT = Path(__file__).parent / "fixtures"
OUT.mkdir(exist_ok=True)
USER_PW = "user-pw-123"
OWNER_PW = "owner-pw-456"
PAGES = 3


def plain_pdf() -> bytes:
    """Hand-written 3-page PDF whose pages say 'Secret page N'."""
    objs = ["<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [%s] /Count %d >>" % (
                " ".join(f"{3 + i * 2} 0 R" for i in range(PAGES)), PAGES)]
    font_id = 3 + PAGES * 2
    for i in range(PAGES):
        stream = f"BT /F1 24 Tf 72 700 Td (Secret page {i + 1}) Tj ET".encode()
        objs.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
                    f"/Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {4 + i * 2} 0 R >>")
        objs.append(f"<< /Length {len(stream)} >>\nstream\n{stream.decode()}\nendstream")
    objs.append("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    out = io.BytesIO()
    out.write(b"%PDF-1.7\n")
    offsets = []
    for n, body in enumerate(objs, 1):
        offsets.append(out.tell())
        out.write(f"{n} 0 obj\n{body}\nendobj\n".encode())
    xref = out.tell()
    out.write(f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode())
    for off in offsets:
        out.write(f"{off:010d} 00000 n \n".encode())
    out.write(f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return out.getvalue()


def locked(base: bytes, user_pw: str, algorithm: str) -> bytes:
    w = PdfWriter(clone_from=PdfReader(io.BytesIO(base)))
    w.encrypt(user_password=user_pw, owner_password=OWNER_PW, algorithm=algorithm,
              permissions_flag=0)  # no print/copy/modify
    buf = io.BytesIO()
    w.write(buf)
    return buf.getvalue()


base = plain_pdf()
assert len(PdfReader(io.BytesIO(base)).pages) == PAGES
(OUT / "plain.pdf").write_bytes(base)

cases = [
    # name, file, password to try, expected outcome
    ("aes256", "aes256.pdf", USER_PW, "unlocked", ("AES-256", USER_PW)),
    ("aes128", "aes128.pdf", USER_PW, "unlocked", ("AES-128", USER_PW)),
    ("rc4-128", "rc4-128.pdf", USER_PW, "unlocked", ("RC4-128", USER_PW)),
    ("owner-only (opens without password)", "owner-only.pdf", "", "unlocked", ("AES-256", "")),
    ("owner password instead of user", "aes256.pdf", OWNER_PW, "unlocked", None),
    ("wrong password", "aes256.pdf", "nope", "wrong-password", None),
    ("no password given", "aes256.pdf", "", "wrong-password", None),
    ("not encrypted", "plain.pdf", "", "unlocked", None),
]
manifest = []
for name, fname, pw, expect, make in cases:
    if make:
        algo, upw = make
        data = locked(base, upw, algo)
        r = PdfReader(io.BytesIO(data))
        assert r.is_encrypted, fname
        (OUT / fname).write_bytes(data)
    manifest.append({"name": name, "file": fname, "password": pw, "expect": expect})
(OUT / "manifest.json").write_text(json.dumps({"pages": PAGES, "cases": manifest}, indent=2))
print("fixtures:", sorted(p.name for p in OUT.iterdir()))
