import hashlib
import json
import runpy
import tempfile
import unittest
from pathlib import Path

module = runpy.run_path(str(Path(__file__).with_name("prepare-gemma-quality-fixtures.py")))
media_photos = module["media_photos"]


class MediaPhotoFixtureTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "photos").mkdir()
        sources = {}
        for category, key, identifier in (("swap_obj", "4", "123"), ("swap_att", "192", "456")):
            path = self.root / f"{category}-media.json"
            path.write_text(json.dumps({
                key: {
                    "filename": f"{int(identifier):012d}.jpg",
                    "caption": "A true caption",
                    "negative_caption": "A false caption",
                }
            }))
            sources[category] = hashlib.sha256(path.read_bytes()).hexdigest()
            (self.root / "photos" / f"{identifier}.jpg").write_bytes(identifier.encode())
        self.sources = sources
        self.original = ("123", hashlib.sha256(b"123").hexdigest(), "author", "https://example.test/123", "CC BY 2.0")
        self.extra = ("456", hashlib.sha256(b"456").hexdigest(), "author", "https://example.test/456", "CC BY-SA 2.0")
        globals_for_function = media_photos.__globals__
        for name, replacement in (
            ("MEDIA_PHOTO_SOURCES", sources),
            ("PHOTOS", {"4": self.original}),
            ("ADDITIONAL_MEDIA_PHOTOS", {("swap_att", "192"): self.extra}),
        ):
            old = globals_for_function[name]
            globals_for_function[name] = replacement
            self.addCleanup(globals_for_function.__setitem__, name, old)

    def test_preserves_independent_pairs_and_attribution(self):
        fixture = media_photos(self.root)
        self.assertEqual([(row["category"], row["pairId"]) for row in fixture], [
            ("swap_obj", "4"), ("swap_att", "192"),
        ])
        self.assertEqual(fixture[1]["negativeCaption"], "A false caption")
        self.assertEqual(fixture[1]["license"], "CC BY-SA 2.0")

    def test_refuses_annotation_drift(self):
        (self.root / "swap_att-media.json").write_text("{}")
        with self.assertRaisesRegex(ValueError, "pinned media annotations differ"):
            media_photos(self.root)

    def test_refuses_photo_drift(self):
        (self.root / "photos" / "456.jpg").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "pinned media photograph differs"):
            media_photos(self.root)

    def test_refuses_duplicate_photo(self):
        media_photos.__globals__["ADDITIONAL_MEDIA_PHOTOS"][("swap_att", "192")] = self.original
        path = self.root / "swap_att-media.json"
        pair = json.loads(path.read_text())
        pair["192"]["filename"] = "000000000123.jpg"
        path.write_text(json.dumps(pair))
        media_photos.__globals__["MEDIA_PHOTO_SOURCES"]["swap_att"] = hashlib.sha256(path.read_bytes()).hexdigest()
        with self.assertRaisesRegex(ValueError, "duplicate media photograph"):
            media_photos(self.root)


if __name__ == "__main__":
    unittest.main()
