import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import manifest


class ManifestTests(unittest.TestCase):
    def test_exact_six_assets_and_hashes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for platform, extension in manifest.PLATFORMS:
                (root / manifest.filename("0.0.1", platform, extension)).write_bytes(platform.encode())
            result = manifest.generate("0.0.1", root)
            self.assertEqual(set(result), {"version", "assets"})
            self.assertEqual(result["version"], "0.0.1")
            self.assertEqual(len(result["assets"]), 6)
            for platform, entry in result["assets"].items():
                self.assertEqual(set(entry), {"filename", "size", "sha256"})
                self.assertEqual(entry["size"], len(platform))
                self.assertEqual(entry["sha256"], hashlib.sha256(platform.encode()).hexdigest())
                self.assertRegex(entry["sha256"], r"^[0-9a-f]{64}$")
                self.assertEqual(entry["filename"], next(
                    manifest.filename("0.0.1", p, ext)
                    for p, ext in manifest.PLATFORMS if p == platform
                ))
            self.assertEqual(json.loads(json.dumps(result)), result)

    def test_rejects_missing_extra_and_empty_assets(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            names = [manifest.filename("1.2.3", p, ext) for p, ext in manifest.PLATFORMS]
            for name in names:
                (root / name).write_bytes(b"test")
            (root / names[0]).unlink()
            with self.assertRaisesRegex(ValueError, "missing="):
                manifest.generate("1.2.3", root)
            (root / names[0]).write_bytes(b"test")
            (root / "unrecognized.exe").write_bytes(b"test")
            with self.assertRaisesRegex(ValueError, "unexpected="):
                manifest.generate("1.2.3", root)
            (root / "unrecognized.exe").unlink()
            (root / names[0]).write_bytes(b"")
            with self.assertRaisesRegex(ValueError, "Empty release asset"):
                manifest.generate("1.2.3", root)

    def test_exact_release_filenames(self):
        expected = (
            "Perch-0.0.1-windows-x86_64-setup.exe",
            "Perch-0.0.1-windows-aarch64-setup.exe",
            "Perch-0.0.1-linux-x86_64.AppImage",
            "Perch-0.0.1-linux-aarch64.AppImage",
            "Perch-0.0.1-macos-x86_64.zip",
            "Perch-0.0.1-macos-aarch64.zip",
        )
        self.assertEqual(tuple(manifest.filename("0.0.1", p, e) for p, e in manifest.PLATFORMS), expected)

    def test_tag_version_and_filename_validation(self):
        with tempfile.TemporaryDirectory() as tmp:
            cargo = Path(tmp) / "Cargo.toml"
            cargo.write_text('[package]\nname = "perch"\nversion = "0.0.1"\n', encoding="utf-8")
            self.assertEqual(manifest.check_version(cargo, "v0.0.1"), "0.0.1")
            with self.assertRaises(ValueError):
                manifest.check_version(cargo, "v0.0.2")
            with self.assertRaises(ValueError):
                manifest.check_version(cargo, "0.0.1")
        for bad in ("v1.2.3", "01.2.3", "1.2.3/../", "1.2.3-beta", "1.2"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                manifest.validate_version(bad)


if __name__ == "__main__":
    unittest.main()
