"""Bundle a native macOS executable as an unsigned, manually installed app."""

import argparse
import plistlib
import re
import shutil
import subprocess
import tempfile
from pathlib import Path


def run(*args):
    subprocess.run(args, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("architecture", choices=("x86_64", "aarch64"))
    parser.add_argument("binary", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", args.version):
        parser.error("Invalid version")
    if not args.binary.is_file() or not args.binary.stat().st_size:
        parser.error("Missing native executable")
    expected = "arm64" if args.architecture == "aarch64" else "x86_64"
    if expected not in subprocess.check_output(("file", "-b", str(args.binary)), text=True):
        parser.error(f"Expected a {expected} Mach-O executable")
    icon = Path(__file__).with_name("perch.png")
    if not icon.is_file():
        parser.error("Missing packaging icon")
    args.output.mkdir(parents=True, exist_ok=True)
    artifact = args.output.resolve() / f"Perch-{args.version}-macos-{args.architecture}.zip"
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        app = tmp / "Perch.app"
        contents = app / "Contents"
        executable = contents / "MacOS" / "perch"
        resources = contents / "Resources"
        executable.parent.mkdir(parents=True)
        resources.mkdir()
        shutil.copy2(args.binary, executable)
        executable.chmod(0o755)
        plist = {
            "CFBundleDevelopmentRegion": "en",
            "CFBundleDisplayName": "Perch",
            "CFBundleExecutable": "perch",
            "CFBundleIconFile": "perch",
            "CFBundleIdentifier": "com.zen-box.perch",
            "CFBundleInfoDictionaryVersion": "6.0",
            "CFBundleName": "Perch",
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": args.version,
            "CFBundleVersion": args.version,
            "LSMinimumSystemVersion": "15.0",
            "NSHighResolutionCapable": True,
        }
        with (contents / "Info.plist").open("wb") as file:
            plistlib.dump(plist, file)
        iconset = tmp / "perch.iconset"
        iconset.mkdir()
        for size in (16, 32, 128, 256, 512):
            run("sips", "-z", str(size), str(size), str(icon), "--out", str(iconset / f"icon_{size}x{size}.png"))
        for size in (16, 32, 128, 256, 512):
            large = size * 2
            run("sips", "-z", str(large), str(large), str(icon), "--out", str(iconset / f"icon_{size}x{size}@2x.png"))
        run("iconutil", "-c", "icns", str(iconset), "-o", str(resources / "perch.icns"))
        run("ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(app), str(artifact))
        check = tmp / "unpacked"
        check.mkdir()
        run("ditto", "-x", "-k", str(artifact), str(check))
        if not (check / "Perch.app/Contents/MacOS/perch").is_file():
            raise RuntimeError("App executable missing from zip")
    if artifact.stat().st_size == 0:
        raise RuntimeError("Empty app archive")
    print(artifact)


if __name__ == "__main__":
    main()
