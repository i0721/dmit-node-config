#!/bin/bash
set -euo pipefail
umask 077
cd /work
bash tests/integration/prepare.sh
python3 tests/integration/probe.py > /root/nodectl-test/probe.log 2>&1 &
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
nodectl export-clients --output /root/nodectl-test/clients >/dev/null
test -f /var/lib/dmit-node/installation.json
test "$(stat -c '%U:%G:%a' /tmp/vless-xhttp.socket)" = xray:www-data:660
curl --fail --silent --http2 --resolve node.integration.test:443:127.0.0.1 https://node.integration.test/ | grep -q integration-ready
curl --fail --silent https://127.0.0.1/ | grep -q integration-ready
ss -H -lun 'sport = :443' | grep -q 443
echo 'SERVICES_TCP_UDP_UDS_H2_THREE_HANDSHAKES_PASS'
if [ "${NODECTL_NETWORK_GATE:-}" = yes ]; then
  touch /root/nodectl-test/network-ready
  for _ in $(seq 1 120); do [ -e /root/nodectl-test/network-verified ] && break; sleep 1; done
  test -e /root/nodectl-test/network-verified
fi
# Change only a real managed setting; snapshots and receipts must differ.
sed -i 's/alt_svc_max_age = 86400/alt_svc_max_age = 3600/' /etc/dmit-node/node.toml
next=$(nodectl render)
check "$next"
for stage in entry-0 entry-1 entry-2 entry-3 entry-4 daemon-reload xray nginx services health; do
  if NODECTL_FAILPOINT="$stage" nodectl apply "$next" >/root/nodectl-test/fault.log 2>&1; then echo "Fault not injected: $stage" >&2; exit 1; fi
  test "$(basename "$(readlink /var/lib/dmit-node/current)")" = "$first"
  test ! -e /var/lib/dmit-node/transaction.json
  systemctl is-active --quiet nginx xray
  nodectl apply "$first" >/dev/null
  echo "RECOVERY_PASS:$stage"
done
if NODECTL_FAILPOINT=journal nodectl apply "$next" >/root/nodectl-test/fault.log 2>&1; then exit 1; fi
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
nodectl rollback
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
echo 'INTEGRATION_PASS'
