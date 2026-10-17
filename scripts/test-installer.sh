#!/bin/bash
# Runs against the real OIDC-signed distribution in the publishing workflow.
set -euo pipefail
bundle=${1:?verified distribution directory}
version=${2:?exact version}
prefix=${3:?private root-owned test installation directory}
negative=${4:?signed negative fixtures directory}
legacy=${5:?binary built from the pinned 0.1 source commit}
script=$(realpath install.sh)
bundle=$(realpath "$bundle")
mkdir -p "$prefix"
mkdir -p "$prefix/bin" "$prefix/legacy-inputs"
test "$("$legacy" --version)" = 'nodectl 0.1.0'
install -m 0755 "$legacy" "$prefix/bin/nodectl"
legacy_hash=$(sha256sum "$prefix/bin/nodectl" | cut -d ' ' -f 1)
if [ -n "${NODECTL_REPO_TOOLS:-}" ]; then
  "$NODECTL_REPO_TOOLS" release-smoke "$prefix/bin/nodectl" "$prefix/legacy-inputs"
else
  cargo run --quiet --locked --bin repo-tools -- release-smoke "$prefix/bin/nodectl" "$prefix/legacy-inputs"
fi
config_hash=$(sha256sum "$prefix/legacy-inputs/config/"* | sha256sum | cut -d ' ' -f 1)
args=(--version "$version" --prefix "$prefix" --bundle-dir "$bundle" --config-dir "$prefix/legacy-inputs/config")
sh "$script" "${args[@]}" --execute
test "$(sha256sum "$prefix/lib/nodectl/versions/$legacy_hash" | cut -d ' ' -f 1)" = "$legacy_hash"
test "$(sha256sum "$prefix/legacy-inputs/config/"* | sha256sum | cut -d ' ' -f 1)" = "$config_hash"
echo 'ACTUAL_0_1_BINARY_AND_INPUT_UPGRADE_PASS'
original=$(sha256sum "$prefix/bin/nodectl" | cut -d ' ' -f 1)
# The same valid signature must be rejected under a different tag identity,
# before any archive can be selected or executed.
if sh "$script" --version v9999.0.0 --prefix "$prefix" --bundle-dir "$bundle" --execute; then echo 'Wrong workflow identity accepted' >&2; exit 1; fi
test "$(sha256sum "$prefix/bin/nodectl" | cut -d ' ' -f 1)" = "$original"
for kind in wrong-arch failing-binary; do
  if sh "$script" --version "$version" --prefix "$prefix" --bundle-dir "$negative/$kind" --config-dir "$prefix/no-config" --execute; then echo "Invalid executable accepted: $kind" >&2; exit 1; fi
  test "$(sha256sum "$prefix/bin/nodectl" | cut -d ' ' -f 1)" = "$original"
done
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
