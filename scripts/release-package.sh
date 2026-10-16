#!/bin/bash
set -euo pipefail
target=${1:?target triple required}
version=$(cargo metadata --no-deps --format-version 1 --locked | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}
commit=${SOURCE_COMMIT:-$(git rev-parse HEAD)}
case "$target" in x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu) :;; *) echo 'Unsupported target' >&2; exit 1;; esac
cargo test --locked --target "$target"
cargo build --release --locked --target "$target"
binary="target/$target/release/nodectl"
"$binary" --help >/dev/null
"$binary" schema > /tmp/nodectl-release-schema.json
diff -u schemas/node.schema.json /tmp/nodectl-release-schema.json
cargo cyclonedx --format json --spec-version 1.5 --target "$target"
out="dist/nodectl-v$version-$target.tar.gz"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
install -m 0755 "$binary" "$work/nodectl"
cp LICENSE THIRD_PARTY.md "$work/"
cp nodectl.cdx.json "$work/sbom.cdx.json"
cargo metadata --locked --format-version 1 --filter-platform "$target" > "$work/cargo-metadata.json"
python3 scripts/release-licenses.py "$work"
printf 'version=%s\ntarget=%s\ncommit=%s\nrust=%s\nsource_date_epoch=%s\n' "$version" "$target" "$commit" "$(rustc --version)" "$SOURCE_DATE_EPOCH" > "$work/BUILD-INFO"
printf 'libc=%s\ncyclonedx=%s\n' "$(getconf GNU_LIBC_VERSION)" "$(cargo cyclonedx --version)" >> "$work/BUILD-INFO"
mkdir -p dist
tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner -C "$work" -czf "$out" nodectl LICENSE THIRD_PARTY.md sbom.cdx.json BUILD-INFO licenses
# Verify the exact archived executable, not just target/release.
tar -xOzf "$out" nodectl > "$work/verify-nodectl"
chmod 0700 "$work/verify-nodectl"
"$work/verify-nodectl" --help >/dev/null
"$work/verify-nodectl" schema | diff -u schemas/node.schema.json -
python3 scripts/release-smoke.py "$work/verify-nodectl"
printf '%s\n' "$out"
