#!/usr/bin/env bash
# Build the romraider-cli .deb (story US-RR-38, task US-RR-38-7;
# INFRASTRUCTURE.md "Linux Release", D-022). Run by job `build-deb` in
# .github/workflows/release.yml; runs the same way locally.
#
#   CARGO_DEB=/path/to/cargo-deb \
#     bash packaging/deb/build.sh <romraider-cli binary> <output directory>
#
# release.yml installs cargo-deb pinned by version (`cargo install --locked`).
# What goes in the package is `[package.metadata.deb]` in
# crates/rr-cli/Cargo.toml: the binary, the desktop entry and the icon.
#
# cargo-deb runs with --no-build on the given binary (the cargo-dist
# tarball's), staged in a temporary target directory as cargo-deb's
# `target/release/romraider-cli`, and --no-strip, so the .deb ships that
# binary byte for byte. The package is named by cargo-deb:
# romraider-cli_<version>-<revision>_<arch>.deb. Its path is printed last.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <romraider-cli binary> <output directory>" >&2
  exit 2
fi
bin=$1
out=$2
cargo_deb=${CARGO_DEB:-cargo-deb}

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../.." && pwd)

# The package version comes from Cargo.toml; the binary must be that version.
reported=$("$bin" --version)
version=${reported#romraider-cli }
manifest=$(cargo metadata --manifest-path "$repo/Cargo.toml" --format-version 1 --no-deps --locked \
  | jq -r '.packages[] | select(.name == "rr-cli") | .version')
if [ "$version" != "$manifest" ]; then
  echo "error: $bin --version printed '$reported', but rr-cli is $manifest" >&2
  exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
install -Dm0755 "$bin" "$work/target/release/romraider-cli"

mkdir -p "$out"
CARGO_TARGET_DIR="$work/target" "$cargo_deb" --manifest-path "$repo/crates/rr-cli/Cargo.toml" \
  --no-build --no-strip --locked --output "$out" > "$work/deb-path"
deb=$(tail -n 1 "$work/deb-path")
test -f "$deb"

echo "Package:" >&2
dpkg-deb --info "$deb" >&2
echo "Contents:" >&2
dpkg-deb --contents "$deb" >&2
echo "$deb"
