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
if sh "$script" --version "$version" --prefix "$prefix" --bundle-dir "$bundle" </dev/null > "$prefix/no-tty.log" 2>&1; then exit 1; fi
grep -q 'requires a terminal' "$prefix/no-tty.log"
test "$(sha256sum "$prefix/bin/nodectl" | cut -d ' ' -f 1)" = "$original"
install -m 0755 "$legacy" "$prefix/bin/nodectl"
if printf 'invalid-domain\n\n\n\n\n\n\n\n\n' | script -q -e -c "sh '$script' --version '$version' --prefix '$prefix' --bundle-dir '$bundle' --config-dir '$prefix/legacy-inputs/config' --state-dir '$prefix/wizard-state'" "$prefix/wizard-failure.log" >/dev/null; then exit 1; fi
grep -q "Installed $version" "$prefix/wizard-failure.log"
grep -q 'original program and deployment preserved' "$prefix/wizard-failure.log"
test "$(sha256sum "$prefix/bin/nodectl" | cut -d ' ' -f 1)" = "$legacy_hash"
test "$(sha256sum "$prefix/legacy-inputs/config/"* | sha256sum | cut -d ' ' -f 1)" = "$config_hash"
sh "$script" "${args[@]}" --execute
test "$(sha256sum "$prefix/bin/nodectl" | cut -d ' ' -f 1)" = "$original"
echo 'SIGNED_WIZARD_FAILURE_RESTORES_OLD_PROGRAM_AND_NO_TTY_REFUSED_PASS'
# A transport interruption after a valid signed manifest must preserve the tool.
mkdir -m 0700 "$prefix/interrupted-download"
cat > "$prefix/interrupted-download/curl" <<'SH'
#!/bin/sh
set -eu
out= name=
while [ "$#" -gt 0 ]; do
  case "$1" in -o) out=$2; shift 2;; https://github.com/*) name=${1##*/}; shift;; *) shift;; esac
done
case "$name" in
  SHA256SUMS|SHA256SUMS.sigstore.json) cp "$NODECTL_SIGNED_TEST_BUNDLE/$name" "$out";;
  nodectl-*.tar.gz) printf interrupted > "$out"; exit 56;;
  *) exit 64;;
esac
SH
chmod 0700 "$prefix/interrupted-download/curl"
if PATH="$prefix/interrupted-download:$PATH" NODECTL_SIGNED_TEST_BUNDLE="$bundle" sh "$script" --version "$version" --prefix "$prefix" --config-dir "$prefix/legacy-inputs/config" --execute > "$prefix/interrupted-download.log" 2>&1; then exit 1; fi
test "$(sha256sum "$prefix/bin/nodectl" | cut -d ' ' -f 1)" = "$original"
echo 'SIGNED_DOWNLOAD_INTERRUPTION_PRESERVES_PROGRAM_PASS'
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
