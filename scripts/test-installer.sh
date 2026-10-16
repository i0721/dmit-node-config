#!/bin/bash
# Runs against the real OIDC-signed distribution in the publishing workflow.
set -euo pipefail
bundle=${1:?verified distribution directory}
version=${2:?exact version}
prefix=${3:?private root-owned test installation directory}
script=$(realpath install.sh)
bundle=$(realpath "$bundle")
mkdir -p "$prefix"
args=(--version "$version" --prefix "$prefix" --bundle-dir "$bundle" --config-dir "$prefix/no-config")
sh "$script" "${args[@]}" --execute
original=$(sha256sum "$prefix/bin/nodectl" | cut -d ' ' -f 1)
sh "$script" "${args[@]}" --execute
test "$(sha256sum "$prefix/lib/nodectl/versions/$original" | cut -d ' ' -f 1)" = "$original"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp "$bundle/"* "$work/"
if [ "$(uname -m)" = x86_64 ]; then target=x86_64; else target=aarch64; fi
archive="nodectl-$version-$target-unknown-linux-gnu.tar.gz"
printf corrupt >> "$work/$archive"
if sh "$script" --version "$version" --prefix "$prefix" --bundle-dir "$work" --config-dir "$prefix/no-config" --execute; then echo 'Corrupt archive accepted' >&2; exit 1; fi
test "$(sha256sum "$prefix/bin/nodectl" | cut -d ' ' -f 1)" = "$original"
cp "$bundle/$archive" "$work/$archive"
printf '\ninvalid\n' >> "$work/SHA256SUMS"
if sh "$script" --version "$version" --prefix "$prefix" --bundle-dir "$work" --execute; then echo 'Invalid signature accepted' >&2; exit 1; fi
test "$(sha256sum "$prefix/bin/nodectl" | cut -d ' ' -f 1)" = "$original"
mkdir -m 0700 "$prefix/config"
printf 'schema_version = 999\n' > "$prefix/config/node.toml"
chmod 0600 "$prefix/config/node.toml"
if sh "$script" --version "$version" --prefix "$prefix" --bundle-dir "$bundle" --config-dir "$prefix/config" --execute; then echo 'Incompatible inputs accepted' >&2; exit 1; fi
test "$(sha256sum "$prefix/bin/nodectl" | cut -d ' ' -f 1)" = "$original"
echo 'SIGNED_INSTALL_UPGRADE_AND_NEGATIVE_GATES_PASS'
