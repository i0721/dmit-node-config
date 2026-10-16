#!/bin/sh
# Installs only nodectl. Dependencies, DNS and certificates are separate steps.
set -eu
umask 077
VERSION= PREFIX=/usr/local EXECUTE=no BUNDLE_DIR= CONFIG_DIR=/etc/dmit-node STATE_DIR=/var/lib/dmit-node
REPOSITORY=i0721/dmit-node-config
usage() { printf '%s\n' 'Usage: install.sh --version vX.Y.Z [--prefix /usr/local] [--execute | --dry-run] [--bundle-dir PATH] [--config-dir PATH] [--state-dir PATH]'; }
die() { printf 'install: %s\n' "$*" >&2; exit 1; }
while [ "$#" -gt 0 ]; do
  case "$1" in
    --version|--prefix|--bundle-dir|--config-dir|--state-dir)
      [ "$#" -ge 2 ] || die "Missing value for $1"
      case "$1" in --version) VERSION=$2;; --prefix) PREFIX=$2;; --bundle-dir) BUNDLE_DIR=$2;; --config-dir) CONFIG_DIR=$2;; --state-dir) STATE_DIR=$2;; esac
      shift 2;;
    --execute) EXECUTE=yes; shift;;
    --dry-run) EXECUTE=no; shift;;
    --help|-h) usage; exit 0;;
    *) die "Unknown option: $1";;
  esac
done
printf '%s\n' "$VERSION" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' || die 'Specify an exact stable version: --version vX.Y.Z'
[ "$(uname -s)" = Linux ] || die 'Linux is required'
case "$(uname -m)" in x86_64|amd64) ARCH=x86_64;; aarch64|arm64) ARCH=aarch64;; *) die 'Unsupported CPU architecture';; esac
for value in "$PREFIX" "$CONFIG_DIR" "$STATE_DIR"; do
  case "$value" in /*) :;; *) die 'Paths must be absolute';; esac
  case "$value" in *'/../'*|*/..|*'/./'*|*/.|/|*'
'*) die 'Unsafe path';; esac
done
ASSET="nodectl-${VERSION}-${ARCH}-unknown-linux-gnu.tar.gz"
IDENTITY="https://github.com/$REPOSITORY/.github/workflows/release.yml@refs/tags/$VERSION"
printf 'Version: %s\nArchitecture: %s\nAsset: %s\nInstall: %s/bin/nodectl\nTrust: %s (GitHub OIDC)\n' "$VERSION" "$ARCH" "$ASSET" "$PREFIX" "$IDENTITY"
[ "$EXECUTE" = yes ] || { printf '%s\n' 'Preview only; rerun with --execute. No files or downloads changed.'; exit 0; }
for tool in curl sha256sum tar cosign mktemp install; do command -v "$tool" >/dev/null 2>&1 || die "Missing $tool; see docs/installation.md"; done
check_dir() {
  [ ! -L "$1" ] || die 'Refusing symbolic-link destination'
  if [ -e "$1" ]; then
    [ -d "$1" ] || die 'Destination parent must be a directory'
    [ "$(stat -c %u "$1")" = "$(id -u)" ] || die 'Destination is not owned by current user'
    [ -z "$(find "$1" -maxdepth 0 -perm /022 -print)" ] || die 'Destination writable by other users'
  fi
}
# Validate every existing path component before creating directories.
check_tree() { p=$1; while [ "$p" != / ]; do check_dir "$p"; p=$(dirname "$p"); done; }
check_tree "$PREFIX"
check_tree "$PREFIX/bin"
check_tree "$PREFIX/lib/nodectl/versions"
mkdir -p "$PREFIX/bin" "$PREFIX/lib/nodectl/versions"
work=$(mktemp -d)
stage=
cleanup() { rm -f "${stage:-$work/unused}"; rm -rf "$work"; }
trap cleanup EXIT HUP INT TERM
get_asset() {
  if [ -n "$BUNDLE_DIR" ]; then
    [ -f "$BUNDLE_DIR/$1" ] && [ ! -L "$BUNDLE_DIR/$1" ] || die 'Missing regular offline asset'
    cp "$BUNDLE_DIR/$1" "$work/$1"
  else
    curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error --max-time 300 "https://github.com/$REPOSITORY/releases/download/$VERSION/$1" -o "$work/$1"
  fi
}
get_asset SHA256SUMS
get_asset SHA256SUMS.sigstore.json
cosign verify-blob --bundle "$work/SHA256SUMS.sigstore.json" --certificate-identity "$IDENTITY" --certificate-oidc-issuer https://token.actions.githubusercontent.com "$work/SHA256SUMS" >/dev/null 2>&1 || die 'Release signature or workflow identity verification failed'
get_asset "$ASSET"
expected=$(awk -v name="$ASSET" '$2 == name { print $1 }' "$work/SHA256SUMS")
[ "${#expected}" = 64 ] || die 'Missing or ambiguous asset digest'
printf '%s\n' "$expected" | grep -Eq '^[0-9a-f]{64}$' || die 'Invalid asset digest'
actual=$(sha256sum "$work/$ASSET" | cut -d ' ' -f 1)
[ "$expected" = "$actual" ] || die 'Archive digest mismatch'
# Extract only the executable; never expand archive paths as root.
[ "$(tar -tzf "$work/$ASSET" nodectl | wc -l)" -eq 1 ] || die 'Archive must contain one nodectl'
tar -xOzf "$work/$ASSET" nodectl > "$work/nodectl"
chmod 0700 "$work/nodectl"
[ "$("$work/nodectl" --version)" = "nodectl ${VERSION#v}" ] || die 'Binary version mismatch'
"$work/nodectl" --help >/dev/null
"$work/nodectl" schema > "$work/schema.json"
if [ -d "$CONFIG_DIR" ]; then "$work/nodectl" --config-dir "$CONFIG_DIR" --state-dir "$STATE_DIR" validate >/dev/null || die 'Existing configuration incompatible; original binary preserved'; fi
dest="$PREFIX/bin/nodectl"
if [ -e "$dest" ] || [ -L "$dest" ]; then
  [ -f "$dest" ] && [ ! -L "$dest" ] || die 'Installed binary must be a regular file'
  [ "$(stat -c %u "$dest")" = "$(id -u)" ] || die 'Installed binary ownership mismatch'
  [ -z "$(find "$dest" -maxdepth 0 -perm /022 -print)" ] || die 'Installed binary is writable by other users'
  old_hash=$(sha256sum "$dest" | cut -d ' ' -f 1)
  backup="$PREFIX/lib/nodectl/versions/$old_hash"
  if [ -e "$backup" ]; then [ "$(sha256sum "$backup" | cut -d ' ' -f 1)" = "$old_hash" ] || die 'Previous binary backup corrupted';
  else install -m 0755 "$dest" "$backup"; fi
  printf 'Previous binary retained: %s\n' "$backup"
fi
stage=$(mktemp "$PREFIX/bin/.nodectl-XXXXXXXX")
install -m 0755 "$work/nodectl" "$stage"
mv -f "$stage" "$dest"
stage=
printf 'Installed %s at %s\n' "$VERSION" "$dest"
