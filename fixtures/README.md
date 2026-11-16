# Golden fixtures

Tests check type, dimensions and size range, never exact bytes.

| File | What it tests | Expected |
| --- | --- | --- |
| `plain.heic` | Normal HEIC, 400×300, red block top-left | Saved, JPEG 400×300 |
| `rotated.heic` | Stored 400×300 with `irot` (from EXIF orientation 6) | Saved, JPEG 300×400, red block top-right, no EXIF |
| `really-heic.jpg` | A HEIC with a `.jpg` name | Saved, with a "really a HEIC" notice |
| `ছবি 😀.heic` | Bengali + emoji file name | Saved as `ছবি 😀 (converted).jpg` |
| `empty.heic` | 0 bytes | Rejected (Corrupt) |
| `truncated.heic` | Cut inside `mdat`, so Probe passes and decoding fails | Failed (Corrupt) |
| `large.heic` | 6000×4000, used for cancel and timings | Saved / Canceled |
| `iphone.heic` | **Missing: add a real iPhone photo** | Saved, size from `iphone.json` |

Regenerate the synthetic files with `python fixtures/make_fixtures.py` (needs `pillow-heif==1.8.1`).

## Adding `iphone.heic`

Use a photo you took yourself (so the licence is clear), preferably portrait so the rotation path is exercised.
Save it as `fixtures/iphone.heic` and record its displayed size in `fixtures/iphone.json`:

```json
{ "width": 3024, "height": 4032 }
```

Until it exists, the iPhone e2e test is skipped (Playwright reports it as skipped, not passed).
