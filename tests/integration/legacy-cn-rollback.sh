#!/bin/bash
# Reproduce a managed historical installation without installation.json. Only
# the explicitly authorized disposable fixture may remove this test baseline.
set -euo pipefail
test -f /.dockerenv
test "${NODECTL_REAL_WARP_TEST:-}" = authorized-two-existing-devices
test "$(nodectl --version)" = 'nodectl 0.5.1'
test "$(/root/nodectl-040 --version)" = 'nodectl 0.4.0'
test ! -e /var/lib/dmit-node/current
umask 077
work=$(mktemp -d /root/nodectl-legacy-cn-XXXXXX)
cp /usr/local/bin/nodectl "$work/new-program"
install -m 755 /root/nodectl-040 /usr/local/bin/nodectl
base=(--non-interactive --skip-dependencies --manual-certificates --warp false --takeover --probe-url https://127.0.0.1:9443/)
/root/nodectl-040 deploy "${base[@]}" > "$work/old-preview.txt"
approval=$(sed -n 's/^Plan SHA256: //p' "$work/old-preview.txt")
test "${#approval}" = 64
/root/nodectl-040 deploy "${base[@]}" --execute --approve "$approval" > "$work/old-apply.txt" 2>&1
test -f /var/lib/dmit-node/installation.json
rm -- /var/lib/dmit-node/installation.json
install -m 755 "$work/new-program" /usr/local/bin/nodectl
test "$(nodectl --version)" = 'nodectl 0.5.1'
original=$(readlink /var/lib/dmit-node/current)
cp /etc/dmit-node/node.toml "$work/old-node.toml"
cp /etc/dmit-node/secrets.json "$work/old-secrets.json"
ip route show default > "$work/default4.txt"
ip -6 route show default > "$work/default6.txt"
ready() {
  test "$(readlink /var/lib/dmit-node/current)" = "$original"
  cmp "$work/old-node.toml" /etc/dmit-node/node.toml
  cmp "$work/old-secrets.json" /etc/dmit-node/secrets.json
  cmp "$work/default4.txt" <(ip route show default)
  cmp "$work/default6.txt" <(ip -6 route show default)
  test ! -e /var/lib/dmit-node/deployment.json
  test ! -e /var/lib/dmit-node/transaction.json
  test ! -e /var/lib/nodectl-web-guard/active
  test ! -e /run/nodectl-cn-policy/owner.json
  test ! -e /var/lib/dmit-node/installation.json
  for entry in /etc/xray/geoip.dat /etc/xray/geosite.dat /etc/systemd/system/usque-cn.service /etc/systemd/system/librespeed.service /etc/systemd/system/openlist.service /var/www/nodectl-services/current; do
    test ! -e "$entry" && test ! -L "$entry"
  done
  systemctl is-active --quiet xray nginx nodectl-share
}
args=(--non-interactive --skip-dependencies --manual-certificates --warp true --cn-warp true --web-services true --probe-url https://127.0.0.1:9443/)
preview() {
  nodectl deploy "${args[@]}" > "$work/new-preview.txt"
  approval=$(sed -n 's/^Plan SHA256: //p' "$work/new-preview.txt")
  test "${#approval}" = 64
}
preview
nodectl deploy "${args[@]}" --execute --approve "$approval" > "$work/new-apply.txt" 2>&1
systemctl is-active --quiet xray nginx nodectl-share usque-warp usque-cn librespeed openlist
test -f /run/nodectl-cn-policy/owner.json
nodectl rollback > "$work/rollback.txt" 2>&1
ready
nodectl doctor --json > "$work/restored-doctor.json"
echo LEGACY_CN_NO_INSTALLATION_BASELINE_COMPLETED_ROLLBACK_RESTORES_040_PASS
preview
if NODECTL_FAILPOINT=cn-warp nodectl deploy "${args[@]}" --execute --approve "$approval" > "$work/failed-apply.txt" 2>&1; then exit 1; fi
grep -q 'Injected integration failure: cn-warp' "$work/failed-apply.txt"
ready
echo LEGACY_CN_NO_INSTALLATION_BASELINE_APPLY_FAILURE_RECOVERY_PASS
echo "Private fixture logs: $work"
