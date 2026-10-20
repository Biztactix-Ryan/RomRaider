#!/usr/bin/env bash
# Build the romraider-cli AppImage (story US-RR-38, task US-RR-38-6;
# INFRASTRUCTURE.md "Linux Release", D-022). Run by job `build-appimage` in
# .github/workflows/release.yml; runs the same way locally.
#
#   APPIMAGETOOL=/path/to/appimagetool-x86_64.AppImage \
#   APPIMAGE_RUNTIME=/path/to/runtime-x86_64 \
#     bash packaging/appimage/build.sh <romraider-cli binary> <output .AppImage>
#
# release.yml downloads both tools pinned by version and SHA-256. The runtime
# is passed with --runtime-file so appimagetool never fetches an unpinned one.
#
# The AppDir is assembled in a temporary directory from the committed skeleton
# packaging/appimage/AppDir/ (AppRun, desktop entry, placeholder icon at the
# root, where appimagetool reads them) plus the binary and license.txt.
# Nothing else goes in: no ROMs, no fixtures (US-RR-38-8 checks the result).
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <romraider-cli binary> <output .AppImage>" >&2
  exit 2
fi
bin=$1
out=$2
: "${APPIMAGETOOL:?set APPIMAGETOOL to the appimagetool AppImage}"
: "${APPIMAGE_RUNTIME:?set APPIMAGE_RUNTIME to the AppImage type2 runtime}"
arch=${ARCH:-x86_64}

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../.." && pwd)
skeleton="$here/AppDir"

# The AppImage carries the binary's own version (X-AppImage-Version).
reported=$("$bin" --version)
version=${reported#romraider-cli }
if [ "$version" = "$reported" ] || [ -z "$version" ]; then
  echo "error: $bin --version printed '$reported', not 'romraider-cli <version>'" >&2
  exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
appdir="$work/romraider-cli.AppDir"

install -Dm0755 "$skeleton/AppRun" "$appdir/AppRun"
install -Dm0644 "$skeleton/romraider-cli.desktop" "$appdir/romraider-cli.desktop"
install -Dm0644 "$skeleton/romraider-cli.svg" "$appdir/romraider-cli.svg"
install -Dm0755 "$bin" "$appdir/usr/bin/romraider-cli"
install -Dm0644 "$skeleton/romraider-cli.desktop" \
  "$appdir/usr/share/applications/romraider-cli.desktop"
install -Dm0644 "$skeleton/romraider-cli.svg" \
  "$appdir/usr/share/icons/hicolor/scalable/apps/romraider-cli.svg"
install -Dm0644 "$repo/license.txt" "$appdir/usr/share/licenses/romraider-cli/license.txt"
ln -s romraider-cli.svg "$appdir/.DirIcon"

echo "AppDir contents:"
(cd "$appdir" && find . -mindepth 1 | LC_ALL=C sort)

mkdir -p "$(dirname "$out")"
rm -f "$out"
# APPIMAGE_EXTRACT_AND_RUN: appimagetool is itself an AppImage; this runs it
# without FUSE (containers, hardened runners).
ARCH=$arch VERSION=$version APPIMAGE_EXTRACT_AND_RUN=1 \
  "$APPIMAGETOOL" --no-appstream --runtime-file "$APPIMAGE_RUNTIME" "$appdir" "$out"
chmod 0755 "$out"
ls -l "$out"
