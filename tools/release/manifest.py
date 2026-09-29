"""Validate release versions and create the public updater manifest (stdlib only)."""

import argparse
import hashlib
import json
import re
import tomllib
from pathlib import Path

PLATFORMS = (
    ("windows-x86_64", "setup.exe"),
    ("windows-aarch64", "setup.exe"),
    ("linux-x86_64", "AppImage"),
    ("linux-aarch64", "AppImage"),
    ("macos-x86_64", "zip"),
    ("macos-aarch64", "zip"),
)
SHA256 = re.compile(r"[0-9a-f]{64}")

def validate_version(value):
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", value):
        raise ValueError(f"Invalid release version: {value!r}")
    return value


def filename(version, platform, extension):
    suffix = f"-setup.exe" if extension == "setup.exe" else f".{extension}"
    return f"Perch-{validate_version(version)}-{platform}{suffix}"


def check_version(cargo_toml, tag):
    with Path(cargo_toml).open("rb") as file:
        version = validate_version(tomllib.load(file)["package"]["version"])
    if tag and tag != f"v{version}":
        raise ValueError(f"Tag {tag!r} must exactly equal Cargo.toml version v{version}")
    return version


def generate(version, directory):
    validate_version(version)
    directory = Path(directory)
    expected = {filename(version, platform, extension) for platform, extension in PLATFORMS}
    actual = {path.name for path in directory.iterdir() if path.is_file()}
    if actual != expected:
        raise ValueError(f"Release asset mismatch: missing={sorted(expected - actual)}, unexpected={sorted(actual - expected)}")
    assets = {}
    for platform, extension in PLATFORMS:
        name = filename(version, platform, extension)
        path = directory / name
        size = path.stat().st_size
        if not size:
            raise ValueError(f"Empty release asset: {name}")
        with path.open("rb") as file:
            digest = hashlib.file_digest(file, "sha256").hexdigest()
        if not SHA256.fullmatch(digest):
            raise ValueError(f"Invalid SHA-256 for {name}")
        assets[platform] = {"filename": name, "size": size, "sha256": digest}
    return {"version": version, "assets": assets}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    check = sub.add_parser("check-version")
    check.add_argument("cargo_toml", type=Path)
    check.add_argument("tag")
    check.add_argument("--github-output", type=Path)
    create = sub.add_parser("generate")
    create.add_argument("version")
    create.add_argument("directory", type=Path)
    create.add_argument("output", type=Path)
    args = parser.parse_args()
    if args.command == "check-version":
        version = check_version(args.cargo_toml, args.tag)
        if args.github_output:
            with args.github_output.open("a", encoding="utf-8") as file:
                file.write(f"version={version}\n")
        print(version)
    else:
        if args.output.parent.resolve() != args.directory.resolve():
            raise ValueError("Manifest must be written next to the release assets")
        manifest = generate(args.version, args.directory)
        args.output.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        print(args.output)


if __name__ == "__main__":
    main()
