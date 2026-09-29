#!/usr/bin/env bash
set -euo pipefail

version="${1:?version}" arch="${2:?arch}" binary="${3:?binary}" output="${4:?output}"
[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || { echo 'Invalid version' >&2; exit 1; }
case "$arch" in x86_64|aarch64) ;; *) echo 'Invalid architecture' >&2; exit 1 ;; esac
[[ -s "$binary" ]] || { echo "Missing native executable: $binary" >&2; exit 1; }
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
[[ -s "$root/tools/release/perch.png" ]] || { echo 'Packaging icon missing' >&2; exit 1; }
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
appdir="$work/Perch.AppDir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/256x256/apps" "$output"
install -m 755 "$binary" "$appdir/usr/bin/perch"
install -m 644 "$root/tools/release/perch.png" "$appdir/perch.png"
install -m 644 "$root/tools/release/perch.png" "$appdir/usr/share/icons/hicolor/256x256/apps/perch.png"
install -m 644 "$root/tools/release/perch.desktop" "$appdir/perch.desktop"
install -m 644 "$root/tools/release/perch.desktop" "$appdir/usr/share/applications/perch.desktop"
install -m 755 "$root/tools/release/AppRun" "$appdir/AppRun"

# linuxdeploy bundles discoverable shared libraries; the clean-container check below rejects missing ones.
ldd "$appdir/usr/bin/perch" | tee "$work/dependencies.txt"
if grep -q 'not found' "$work/dependencies.txt"; then
  echo 'Missing runtime dependencies (see ldd output above); refusing to publish this AppImage.' >&2
  exit 1
fi

mkdir -p "$work/tools" "$work/bundle"
url="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-${arch}.AppImage"
curl --fail --location --retry 3 "$url" --output "$work/tools/linuxdeploy.AppImage"
chmod +x "$work/tools/linuxdeploy.AppImage"
# linuxdeploy deliberately excludes some common desktop libraries. A minimal Ubuntu
# image may not have them, so explicitly include these rather than weakening the
# clean-container dependency check after packaging.
libraries=()
for name in libxcb.so.1 libfontconfig.so.1 libfreetype.so.6 libexpat.so.1; do
  library="$(awk -v name="$name" '$1 == name && $2 == "=>" { print $3; exit }' "$work/dependencies.txt")"
  if [[ "$name" == libexpat.so.1 && -z "$library" ]]; then
    # Expat is pulled in by fontconfig, not necessarily by the Perch binary itself.
    library="$(ldconfig -p | awk -v name="$name" '$1 == name { print $NF; exit }')"
  fi
  if [[ "$name" == libexpat.so.1 && ! -f "$library" ]]; then
    echo 'libexpat.so.1 is required for the bundled fontconfig library' >&2
    exit 1
  fi
  if [[ -n "$library" ]]; then
    [[ -f "$library" ]] || { echo "Required library not found: $name" >&2; exit 1; }
    libraries+=(--library "$library")
  fi
done
(cd "$work/bundle" && ARCH="$arch" LINUXDEPLOY_OUTPUT_VERSION="$version" APPIMAGE_EXTRACT_AND_RUN=1 "$work/tools/linuxdeploy.AppImage" --appdir "$appdir" "${libraries[@]}" --output appimage)
mapfile -t built < <(find "$work/bundle" -maxdepth 1 -type f -name '*.AppImage' -print)
[[ "${#built[@]}" -eq 1 ]] || { echo 'Expected exactly one linuxdeploy AppImage' >&2; exit 1; }
artifact="$(cd "$output" && pwd)/Perch-${version}-linux-${arch}.AppImage"
cp "${built[0]}" "$artifact"
chmod +x "$artifact"
[[ -s "$artifact" ]] || { echo 'AppImage missing or empty' >&2; exit 1; }
APPIMAGE_EXTRACT_AND_RUN=1 "$artifact" --appimage-help
(cd "$work/bundle" && "$artifact" --appimage-extract >/dev/null)
docker run --rm --network none --mount "type=bind,src=$work/bundle/squashfs-root,dst=/app,readonly" ubuntu:24.04 \
  sh -c 'LD_LIBRARY_PATH=/app/usr/lib:/app/usr/lib/x86_64-linux-gnu:/app/usr/lib/aarch64-linux-gnu ldd /app/usr/bin/perch | tee /tmp/deps; ! grep -q "not found" /tmp/deps'
