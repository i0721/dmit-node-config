#!/bin/bash
set -euo pipefail
umask 077
cd /work
bash tests/integration/prepare.sh
/integration-target/debug/repo-tools probe > /root/nodectl-test/probe.log 2>&1 &
probe=$!
trap 'kill "$probe" 2>/dev/null || true' EXIT
check() {
  nodectl check "$1" --client /usr/local/bin/xray --client-sha256 "$(sha256sum /usr/local/bin/xray | cut -d ' ' -f 1)" --client-version 26.10.10 --probe-url https://127.0.0.1:9443/ >/dev/null
}
first=$(nodectl render)
check "$first"
if nodectl apply "$first" > /root/nodectl-test/no-takeover.log 2>&1; then echo 'Unmanaged service overwritten' >&2; exit 1; fi
nodectl takeover "$first" >/dev/null
if ! nodectl apply "$first"; then
  curl --fail --silent --show-error --max-time 5 https://probe.integration.test:9443/ >/dev/null || true
  tail -8 /root/nodectl-test/probe.log
  exit 1
fi
nodectl doctor
nodectl export-clients --format all --output /root/nodectl-test/clients >/dev/null
bash tests/integration/sharing.sh
bash tests/integration/sharing-provider.sh
test -f /var/lib/dmit-node/installation.json
test "$(stat -c '%U:%G:%a' /tmp/vless-xhttp.socket)" = xray:www-data:660
curl --fail --silent --http2 --resolve node.integration.test:443:127.0.0.1 https://node.integration.test/ | grep -q integration-ready
curl --fail --silent https://127.0.0.1/ | grep -q integration-ready
ss -H -lun 'sport = :443' | grep -q 443
echo 'SERVICES_TCP_UDP_UDS_H2_THREE_HANDSHAKES_PASS'
if [ "${NODECTL_NETWORK_GATE:-}" = yes ]; then
  touch /root/nodectl-test/network-ready
  for _ in $(seq 1 900); do [ -e /root/nodectl-test/network-verified ] && break; sleep 1; done
  test -e /root/nodectl-test/network-verified
fi
# Change only a real managed setting; snapshots and receipts must differ.
sed -i 's/alt_svc_max_age = 86400/alt_svc_max_age = 3600/' /etc/dmit-node/node.toml
next=$(nodectl render)
check "$next"
for stage in entry-0 entry-1 entry-2 entry-3 entry-4 daemon-reload sharing xray nginx services health; do
  if NODECTL_FAILPOINT="$stage" nodectl apply "$next" >/root/nodectl-test/fault.log 2>&1; then echo "Fault not injected: $stage" >&2; exit 1; fi
  grep -q "Injected integration failure: $stage" /root/nodectl-test/fault.log || { cat /root/nodectl-test/fault.log; exit 1; }
  test "$(basename "$(readlink /var/lib/dmit-node/current)")" = "$first"
  test ! -e /var/lib/dmit-node/transaction.json
  systemctl is-active --quiet nginx xray
  nodectl apply "$first" >/dev/null
  echo "RECOVERY_PASS:$stage"
done
if NODECTL_FAILPOINT=journal nodectl apply "$next" >/root/nodectl-test/fault.log 2>&1; then exit 1; fi
grep -q 'Injected integration failure: journal' /root/nodectl-test/fault.log || { cat /root/nodectl-test/fault.log; exit 1; }
test -f /var/lib/dmit-node/transaction.json
if nodectl render >/root/nodectl-test/locked.log 2>&1; then echo 'Pending transaction not locked' >&2; exit 1; fi
nodectl recover
test ! -e /var/lib/dmit-node/transaction.json
if NODECTL_FAILPOINT=entry-1,restore-entry-0 nodectl apply "$next" >/root/nodectl-test/recovery-fault.log 2>&1; then exit 1; fi
test -f /var/lib/dmit-node/transaction.json
nodectl recover
nodectl apply "$first" >/dev/null
echo 'RECOVERY_FAILURE_RETRY_PASS'
nodectl apply "$next"
bash tests/integration/sharing.sh verify "$next"
nodectl rollback
bash tests/integration/sharing.sh verify "$first"
test "$(basename "$(readlink /var/lib/dmit-node/current)")" = "$first"
if NODECTL_FAILPOINT=uninstall-journal nodectl uninstall --execute >/root/nodectl-test/uninstall-fault.log 2>&1; then exit 1; fi
test -f /var/lib/dmit-node/uninstall.json
nodectl recover
test ! -e /var/lib/dmit-node/uninstall.json
nodectl export-clients --output /root/nodectl-test/recovered-clients >/dev/null
echo 'UNINSTALL_INTERRUPTION_RECOVERY_PASS'
# Uninstall must refuse incomplete history and drift without changing services.
mv /var/lib/dmit-node/installation.json /root/nodectl-test/installation.json
if nodectl uninstall --execute >/root/nodectl-test/no-baseline.log 2>&1; then exit 1; fi
mv /root/nodectl-test/installation.json /var/lib/dmit-node/installation.json
mv /etc/nginx/nginx.conf /root/nodectl-test/managed-nginx-link
printf 'operator drift\n' > /etc/nginx/nginx.conf
if nodectl uninstall --execute >/root/nodectl-test/drift.log 2>&1; then exit 1; fi
grep -q 'operator drift' /etc/nginx/nginx.conf
mv -f /root/nodectl-test/managed-nginx-link /etc/nginx/nginx.conf
systemctl is-active --quiet nginx xray
echo 'UNINSTALL_MISSING_BASELINE_AND_DRIFT_REFUSED_PASS'
# Fail both the original restore and its rollback, then retry via recover.
if NODECTL_FAILPOINT=restore-entry-0 nodectl uninstall --execute >/root/nodectl-test/uninstall-restore-fault.log 2>&1; then exit 1; fi
test -f /var/lib/dmit-node/uninstall.json
test -f /var/lib/dmit-node/transaction.json
nodectl recover
test ! -e /var/lib/dmit-node/uninstall.json
test ! -e /var/lib/dmit-node/transaction.json
nodectl apply "$first" >/dev/null
echo 'UNINSTALL_FAILED_RESTORE_RECOVERY_PASS'
if NODECTL_FAILPOINT=uninstall-restored nodectl uninstall --execute >/root/nodectl-test/uninstall-commit-fault.log 2>&1; then exit 1; fi
test ! -e /var/lib/dmit-node/current
test -f /var/lib/dmit-node/uninstall.json
nodectl recover
nodectl export-clients --output /root/nodectl-test/uninstall-commit-recovered-clients >/dev/null
echo 'UNINSTALL_COMPLETED_RESTORE_INTERRUPTION_RECOVERY_PASS'
nodectl uninstall
nodectl uninstall --execute
cmp /etc/nginx/nginx.conf /root/nodectl-test/original-nginx.conf
systemctl is-active --quiet nginx
systemctl is-enabled --quiet nginx
test ! -e /var/lib/dmit-node/current
test -f /etc/dmit-node/secrets.json
test -f /etc/ssl/dmit/domain/privkey.pem
test -d /var/lib/dmit-node/backups
curl --fail --silent http://127.0.0.1:9081/ | grep -q legacy-service
echo 'UPGRADE_CONFIG_ROLLBACK_UNINSTALL_RESTORE_PASS'

# Exercise explicit legacy-to-encrypted migration, including key generation and
# restoration of the original inputs that contained no encryption material.
/integration-target/debug/repo-tools legacy-inputs-fixture /etc/dmit-node

# The automatic path uses the same real systemd/services/CA and a local DNS fixture.
ip4=$(ip -4 -j address show dev eth0 | /integration-target/debug/repo-tools ipv4)
dnsmasq --no-daemon --conf-file=/dev/null --port=1053 --listen-address=127.0.0.1 --bind-interfaces --no-resolv --no-hosts --address="/.integration.test/$ip4" --address=/.integration.test/::1 > /root/nodectl-test/dns.log 2>&1 &
dns=$!
trap 'kill "$probe" "$dns" 2>/dev/null || true' EXIT
export NODECTL_TEST_DNS=yes
for _ in $(seq 1 20); do [ "$(dig +short -p 1053 @127.0.0.1 node.integration.test A)" = "$ip4" ] && break; sleep 1; done
test "$(dig +short -p 1053 @127.0.0.1 node.integration.test A)" = "$ip4"
auto=(--non-interactive --skip-dependencies --manual-certificates --takeover --probe-url https://127.0.0.1:9443/)
nodectl deploy "${auto[@]}" > /root/nodectl-test/deploy-preview.txt
approved=$(sed -n 's/^Plan SHA256: //p' /root/nodectl-test/deploy-preview.txt)
test "${#approved}" = 64
secret_before=$(sha256sum /etc/dmit-node/secrets.json)
identity_before=$(/integration-target/debug/repo-tools json-digest /etc/dmit-node/secrets.json)
if nodectl deploy "${auto[@]}" --execute --approve invalid > /root/nodectl-test/stale-approval.log 2>&1; then echo 'Invalid approval accepted' >&2; exit 1; fi
test ! -e /var/lib/dmit-node/deployment.json
nodectl deploy "${auto[@]}" --execute --approve "$approved" > /root/nodectl-test/deploy-execute.txt
test "$(/integration-target/debug/repo-tools json-digest /etc/dmit-node/secrets.json)" = "$identity_before"
secret_before=$(sha256sum /etc/dmit-node/secrets.json)
auto_current=$(basename "$(readlink /var/lib/dmit-node/current)")
printf '<!doctype html><html><meta charset="utf-8"><title>DMIT</title><h1>DMIT node</h1></html>\n' > /var/www/html/index.html
cp /var/www/html/index.html /root/nodectl-test/site-before-migration.html
cp /etc/ssl/dmit/domain/fullchain.pem /root/nodectl-test/domain-before-migration.pem
install -d -m 0700 /root/nodectl-test/two-name-cert
cp /etc/ssl/dmit/domain/privkey.pem /root/nodectl-test/two-name-cert/privkey.pem
openssl req -new -key /etc/ssl/dmit/domain/privkey.pem -out /root/nodectl-test/two-name.csr -subj /CN=speedtest.integration.test >/dev/null 2>&1
printf 'subjectAltName=DNS:speedtest.integration.test,DNS:files.integration.test\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n' > /root/nodectl-test/two-name.extensions
openssl x509 -req -in /root/nodectl-test/two-name.csr -CA /usr/local/share/ca-certificates/nodectl-test.crt -CAkey /root/nodectl-test/ca.key -CAcreateserial -out /root/nodectl-test/two-name-cert/fullchain.pem -days 3 -extfile /root/nodectl-test/two-name.extensions >/dev/null 2>&1
chmod 0600 /root/nodectl-test/two-name-cert/*
sed 's/alt_svc_max_age = 3600/alt_svc_max_age = 7200/' /etc/dmit-node/node.toml > /root/nodectl-test/candidate.toml
chmod 0600 /root/nodectl-test/candidate.toml
auto+=(--input /root/nodectl-test/candidate.toml --domain speedtest.integration.test --reality-domain files.integration.test --replace-domain-set --vless-encryption true)
nodectl deploy "${auto[@]}" > /root/nodectl-test/deploy-preview.txt
approved=$(sed -n 's/^Plan SHA256: //p' /root/nodectl-test/deploy-preview.txt)
if NODECTL_DEPLOY_FAILPOINT=certificate-material NODECTL_TEST_CERTIFICATE_DIR=/root/nodectl-test/two-name-cert nodectl deploy "${auto[@]}" --execute --approve "$approved" > /root/nodectl-test/material-failure.txt 2>&1; then exit 1; fi
grep -q 'Injected deployment certificate material interruption' /root/nodectl-test/material-failure.txt || { cat /root/nodectl-test/material-failure.txt; exit 1; }
test ! -e /var/lib/dmit-node/deployment.json
cmp /etc/ssl/dmit/domain/fullchain.pem /root/nodectl-test/domain-before-migration.pem
cmp /var/www/html/index.html /root/nodectl-test/site-before-migration.html
test "$(sha256sum /etc/dmit-node/secrets.json)" = "$secret_before"
echo DOMAIN_CERTIFICATE_HOMEPAGE_INPUT_FAILURE_RESTORE_PASS
nodectl deploy "${auto[@]}" > /root/nodectl-test/deploy-preview.txt
approved=$(sed -n 's/^Plan SHA256: //p' /root/nodectl-test/deploy-preview.txt)
if NODECTL_FAILPOINT=nginx NODECTL_TEST_CERTIFICATE_DIR=/root/nodectl-test/two-name-cert nodectl deploy "${auto[@]}" --execute --approve "$approved" > /root/nodectl-test/deploy-failure.txt 2>&1; then echo 'Automatic failure not injected' >&2; exit 1; fi
grep -q 'Injected integration failure: nginx' /root/nodectl-test/deploy-failure.txt || { cat /root/nodectl-test/deploy-failure.txt; exit 1; }
test ! -e /var/lib/dmit-node/deployment.json
test "$(basename "$(readlink /var/lib/dmit-node/current)")" = "$auto_current"
grep -q 'alt_svc_max_age = 3600' /etc/dmit-node/node.toml
systemctl is-active --quiet nginx xray
nodectl deploy "${auto[@]}" > /root/nodectl-test/deploy-preview.txt
approved=$(sed -n 's/^Plan SHA256: //p' /root/nodectl-test/deploy-preview.txt)
if NODECTL_DEPLOY_FAILPOINT=journal nodectl deploy "${auto[@]}" --execute --approve "$approved" > /root/nodectl-test/deploy-interrupted.txt 2>&1; then exit 1; fi
test -f /var/lib/dmit-node/deployment.json
if nodectl render > /root/nodectl-test/deploy-blocked.txt 2>&1; then echo 'Provisioning journal bypassed' >&2; exit 1; fi
nodectl recover
test ! -e /var/lib/dmit-node/deployment.json
test "$(sha256sum /etc/dmit-node/secrets.json)" = "$secret_before"
nodectl deploy "${auto[@]}" > /root/nodectl-test/deploy-preview.txt
approved=$(sed -n 's/^Plan SHA256: //p' /root/nodectl-test/deploy-preview.txt)
NODECTL_TEST_CERTIFICATE_DIR=/root/nodectl-test/two-name-cert nodectl deploy "${auto[@]}" --execute --approve "$approved" > /root/nodectl-test/deploy-upgrade.txt
grep -q 'alt_svc_max_age = 7200' /etc/dmit-node/node.toml
grep -q '<title>Welcome</title>' /var/www/html/index.html
nodectl renew-hook
# Managed entrypoint drift must be refused before restoring any materials.
mv /etc/nginx/nginx.conf /root/nodectl-test/migration-nginx-link
printf 'operator drift\n' > /etc/nginx/nginx.conf
if nodectl rollback > /root/nodectl-test/migration-rollback-drift.txt 2>&1; then exit 1; fi
test ! -e /var/lib/dmit-node/deployment.json
cmp /etc/ssl/dmit/domain/fullchain.pem /root/nodectl-test/two-name-cert/fullchain.pem
grep -q '<title>Welcome</title>' /var/www/html/index.html
grep -q 'alt_svc_max_age = 7200' /etc/dmit-node/node.toml
mv -f /root/nodectl-test/migration-nginx-link /etc/nginx/nginx.conf
echo COMPLETED_ROLLBACK_ENTRYPOINT_DRIFT_REFUSED_BEFORE_MUTATION_PASS
nodectl rollback
test "$(basename "$(readlink /var/lib/dmit-node/current)")" = "$auto_current"
cmp /etc/ssl/dmit/domain/fullchain.pem /root/nodectl-test/domain-before-migration.pem
cmp /var/www/html/index.html /root/nodectl-test/site-before-migration.html
test "$(sha256sum /etc/dmit-node/secrets.json)" = "$secret_before"
echo COMPLETED_DOMAIN_CERTIFICATE_HOMEPAGE_RENEWAL_ROLLBACK_PASS
nodectl uninstall --execute
curl --fail --silent http://127.0.0.1:9081/ | grep -q legacy-service
echo 'AUTOMATED_PREVIEW_APPROVAL_IDENTITY_UPGRADE_FAILURE_RECOVERY_PASS'
# Exercise the actual terminal path: cancel after review, then deploy and export
# in one session without retyping a digest or persisting preview credentials.
wizard='nodectl deploy --wizard --skip-dependencies --manual-certificates --takeover --probe-url https://127.0.0.1:9443/'
answers() { printf '\n\n\n\n\n\n\n\n\nfalse\nmanual\n\n\n%s\n' "$1"; }
before=$(sha256sum /etc/dmit-node/node.toml /etc/dmit-node/secrets.json)
if answers no | script -q -e -c "$wizard" /root/nodectl-test/wizard-cancel.log >/dev/null; then exit 1; fi
grep -q 'Deployment cancelled' /root/nodectl-test/wizard-cancel.log
test "$before" = "$(sha256sum /etc/dmit-node/node.toml /etc/dmit-node/secrets.json)"
test ! -e /var/lib/dmit-node/current
answers yes | script -q -e -c "$wizard" /root/nodectl-test/wizard-success.log >/dev/null
grep -q 'Private Xray/Mihomo clients and weights:' /root/nodectl-test/wizard-success.log
find /var/lib/dmit-node/client-exports -name proxies.yaml | grep -q .
nodectl uninstall --execute >/dev/null
echo WIZARD_CANCEL_AND_SINGLE_SESSION_DEPLOY_EXPORT_PASS
NODECTL_ISOLATED_ACME=yes cargo test --locked --features integration-test --target-dir /integration-target isolated_acme_preparation -- --ignored --nocapture --test-threads=1
echo 'INTEGRATION_PASS'
