#!/bin/bash
# Disposable fixture with v0.4.0 tag source built with integration-test and
# already applied. This trusts the fixture CA; it is not a signed release test.
# No WARP/DNS/CA registration: website upgrade and legacy restore only.
set -euo pipefail
umask 077
test "$(nodectl --version)" = 'nodectl 0.4.0'
test -x /root/nodectl-test/candidate-050
work=$(mktemp -d /root/nodectl-upgrade-040-XXXXXX)
old=$(basename "$(readlink /var/lib/dmit-node/current)")
before=$(sha256sum /etc/dmit-node/secrets.json /etc/ssl/dmit/{domain,ip}/{fullchain,privkey}.pem)
ip -j route show table main > "$work/main.json"
nodectl export-clients --format all --output "$work/old-clients" >/dev/null
cp /etc/dmit-node/node.toml "$work/old.toml"
cp /usr/local/bin/nodectl "$work/old-program"
install -m 0755 /root/nodectl-test/candidate-050 /usr/local/bin/nodectl
test "$(nodectl --version)" = 'nodectl 0.5.0'
# Diff verifies the old snapshot with its original renderer and full digest.
nodectl diff "$old" > "$work/old-diff.txt"
args=(--non-interactive --skip-dependencies --manual-certificates --takeover --warp false --web-services true --probe-url https://127.0.0.1:9443/)
preview() {
  nodectl deploy "${args[@]}" > "$work/preview.txt"
  approval=$(sed -n 's/^Plan SHA256: //p' "$work/preview.txt")
  test "${#approval}" = 64
  grep -q 'preserved_and_live_test_required' "$work/preview.txt"
}
intact() {
  test "$before" = "$(sha256sum /etc/dmit-node/secrets.json /etc/ssl/dmit/{domain,ip}/{fullchain,privkey}.pem)"
  cmp "$work/main.json" <(ip -j route show table main)
  test ! -e /var/lib/dmit-node/deployment.json
  test ! -e /var/lib/dmit-node/transaction.json
  test ! -e /var/lib/nodectl-web-guard/active
  systemctl is-active --quiet nginx xray
}
preview
if NODECTL_FAILPOINT=web-services nodectl deploy "${args[@]}" --execute --approve "$approval" > "$work/failure.txt" 2>&1; then exit 1; fi
grep -q 'Injected integration failure: web-services' "$work/failure.txt"
test "$(basename "$(readlink /var/lib/dmit-node/current)")" = "$old"
cmp "$work/old.toml" /etc/dmit-node/node.toml
intact
echo HISTORICAL_040_TO_050_WEBSITE_FAILURE_RESTORES_LEGACY_CONFIG_AND_SERVICES_PASS
preview
nodectl deploy "${args[@]}" --execute --approve "$approval" > "$work/success.txt" 2>&1
intact
test "$(basename "$(readlink /var/lib/dmit-node/current)")" != "$old"
systemctl is-active --quiet openlist librespeed
nodectl export-clients --format all --output "$work/new-clients" >/dev/null
for file in profiles.json proxies.yaml weights.json; do cmp "$work/old-clients/$file" "$work/new-clients/$file"; done
curl --noproxy '*' -fsS https://node.integration.test/ | grep -q 'Network Lab'
curl --noproxy '*' -fsS https://reality.integration.test/api/public/settings >/dev/null
nodectl renew-hook > "$work/renew.txt" 2>&1
intact
echo HISTORICAL_040_TO_050_REAL_WEBSITES_CLIENT_BYTES_IDENTITIES_CERTS_ROUTES_RENEW_HOOK_PASS
runuser -u nodectl-files -- sh -c 'printf "retained upgrade payload\n" > /var/lib/nodectl-openlist-files/legacy-upgrade-preserved.txt'
payload=$(sha256sum /var/lib/nodectl-openlist-files/legacy-upgrade-preserved.txt)
# A restored, journal-verified sharing link must not make arbitrary sharing
# entrypoint drift acceptable for an ordinary completed rollback.
new=$(basename "$(readlink /var/lib/dmit-node/current)")
mv /etc/systemd/system/nodectl-share.service "$work/managed-share-link"
printf 'unmanaged drift fixture\n' > /etc/systemd/system/nodectl-share.service
if nodectl rollback > "$work/share-drift.txt" 2>&1; then exit 1; fi
grep -q 'Deployment materials changed after completion' "$work/share-drift.txt"
test "$(basename "$(readlink /var/lib/dmit-node/current)")" = "$new"
test ! -e /var/lib/dmit-node/deployment.json
rm /etc/systemd/system/nodectl-share.service
mv "$work/managed-share-link" /etc/systemd/system/nodectl-share.service
echo HISTORICAL_UPGRADE_SHARING_ENTRY_DRIFT_REFUSED_BEFORE_MUTATION_PASS
nodectl rollback > "$work/rollback.txt" 2>&1
intact
test "$(basename "$(readlink /var/lib/dmit-node/current)")" = "$old"
cmp "$work/old.toml" /etc/dmit-node/node.toml
nodectl export-clients --format all --output "$work/restored-clients" >/dev/null
for file in profiles.json proxies.yaml weights.json; do cmp "$work/old-clients/$file" "$work/restored-clients/$file"; done
curl --noproxy '*' -fsS https://node.integration.test/ | grep -q integration-ready
for service in openlist librespeed; do if systemctl is-active --quiet "$service"; then exit 1; fi; done
test "$payload" = "$(sha256sum /var/lib/nodectl-openlist-files/legacy-upgrade-preserved.txt)"
echo HISTORICAL_040_TO_050_FULL_LEGACY_ROLLBACK_UPLOADED_FILES_RETAINED_PASS
echo "Private fixture logs: $work"
