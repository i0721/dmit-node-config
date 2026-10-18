#!/bin/bash
# Disposable systemd fixture only. Official native binaries must already exist.
set -euo pipefail
umask 077
work=$(mktemp -d /root/nodectl-web-lifecycle-XXXXXX)
test ! -e /var/lib/dmit-node/current
args=(--non-interactive --manual-certificates --takeover --warp false --web-services true --distribution true --domain speedtest.integration.test --reality-domain files.integration.test --replace-domain-set --probe-url https://127.0.0.1:9443/)
preview() {
  NODECTL_TEST_DNS=yes nodectl deploy "${args[@]}" "$@" > "$work/preview.log"
  approval=$(sed -n 's/^Plan SHA256: //p' "$work/preview.log")
  test "${#approval}" = 64
}
apply() { NODECTL_TEST_DNS=yes nodectl deploy "${args[@]}" "$@" --execute --approve "$approval"; }
preview
apply > "$work/initial.log" 2>&1
echo NATIVE_WEBSITE_AUTOMATIC_DEPENDENCIES_AND_DEPLOY_PASS
private_before=$(sha256sum /etc/dmit-node/secrets.json /etc/nodectl-services/openlist-initial.env)
runuser -u nodectl-files -- sh -c 'printf "preserved uploaded payload\n" > /var/lib/nodectl-openlist-files/recovery-payload.txt'
payload_before=$(sha256sum /var/lib/nodectl-openlist-files/recovery-payload.txt)
metadata_before=$(stat -c '%u:%g:%a' /var/lib/nodectl-openlist-files)
ready() {
  systemctl is-active --quiet nginx xray openlist librespeed nodectl-share
  test ! -e /var/lib/dmit-node/deployment.json
  test ! -e /var/lib/dmit-node/transaction.json
  test ! -e /var/lib/nodectl-web-guard/active
  test "$private_before" = "$(sha256sum /etc/dmit-node/secrets.json /etc/nodectl-services/openlist-initial.env)"
  test "$payload_before" = "$(sha256sum /var/lib/nodectl-openlist-files/recovery-payload.txt)"
  test "$metadata_before" = "$(stat -c '%u:%g:%a' /var/lib/nodectl-openlist-files)"
  curl --noproxy '*' -fSs https://speedtest.integration.test/ | grep -q 'Network Lab'
  curl --noproxy '*' -fSs https://files.integration.test/api/public/settings > /dev/null
}
ready
# Test all nested OpenList handlers, while keeping node routes independent.
test ! -e /var/lib/nodectl-web-guard/active
touch /var/lib/nodectl-web-guard/active
trap 'rm -f /var/lib/nodectl-web-guard/active' EXIT
for route in / /api/public/settings /api/auth/login /p/recovery-payload.txt /dav/; do
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' "https://files.integration.test$route")
  test "$status" = 503
done
rm /var/lib/nodectl-web-guard/active
trap - EXIT
echo FILES_NESTED_API_DOWNLOAD_DAV_MAINTENANCE_BLOCK_PASS
number=3700
for stage in website-snapshot journal; do
  original=$(readlink /var/lib/dmit-node/current)
  cp /etc/dmit-node/node.toml "$work/candidate.toml"
  sed -i "s/^alt_svc_max_age *=.*/alt_svc_max_age = $number/" "$work/candidate.toml"
  number=$((number+1))
  preview --input "$work/candidate.toml"
  if NODECTL_DEPLOY_FAILPOINT="$stage" apply --input "$work/candidate.toml" > "$work/$stage.log" 2>&1; then echo "Interruption not injected: $stage" >&2; exit 1; fi
  test -f /var/lib/dmit-node/deployment.json
  test -f /var/lib/nodectl-web-guard/active
  nodectl recover > "$work/$stage-recovery.log" 2>&1
  test "$(readlink /var/lib/dmit-node/current)" = "$original"
  ready
  echo "WEBSITE_INTERRUPTED_MATERIAL_AND_SERVICE_RECOVERY_PASS:$stage"
done
original=$(readlink /var/lib/dmit-node/current)
preview --input "$work/candidate.toml"
if NODECTL_FAILPOINT=web-services apply --input "$work/candidate.toml" > "$work/apply-failure.log" 2>&1; then echo 'Website apply failure not injected' >&2; exit 1; fi
grep -q 'Injected integration failure: web-services' "$work/apply-failure.log"
test "$(readlink /var/lib/dmit-node/current)" = "$original"
ready
echo WEBSITE_APPLY_FAILURE_AUTOMATIC_DATABASE_AND_HOST_RECOVERY_PASS
preview --input "$work/candidate.toml"
if NODECTL_DEPLOY_FAILPOINT=website-committed apply --input "$work/candidate.toml" > "$work/committed.log" 2>&1; then echo 'Commit interruption not injected' >&2; exit 1; fi
grep -q '"phase": "committed"' /var/lib/dmit-node/deployment.json
test "$(readlink /var/lib/dmit-node/current)" != "$original"
printf 'payload written after durable commit\n' >> /var/lib/nodectl-openlist-files/recovery-payload.txt
payload_before=$(sha256sum /var/lib/nodectl-openlist-files/recovery-payload.txt)
nodectl recover > "$work/committed-recovery.log" 2>&1
ready
echo WEBSITE_COMMITTED_RECOVERY_FINALIZES_WITHOUT_REPLAYING_OLD_DATA_PASS
echo "Private fixture logs: $work"
