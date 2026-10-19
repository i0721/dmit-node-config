#!/bin/bash
# Only in a disposable labelled systemd container; no production credentials.
set -euo pipefail
umask 077
test "${NODECTL_ADMIN_TEST:-}" = isolated
port=${NODECTL_ADMIN_TEST_PORT:-10443}
[[ "$port" =~ ^[0-9]+$ ]] && [ "$port" -ge 1024 ] && [ "$port" -lt 65535 ] || exit 1
test -d /run/systemd/system
test -f /opt/nodectl-admin-test-ui/index.html
cd /work
bash tests/integration/prepare.sh
NODECTL_ADMIN_TEST=isolated bash tests/integration/admin-localhost.sh
/integration-target/debug/repo-tools probe > /root/nodectl-test/admin-probe.log 2>&1 &
printf '%s\n' "$!" > /root/nodectl-test/admin-probe.pid
release=$(nodectl render)
nodectl check "$release" --client /usr/local/bin/xray --client-sha256 "$(sha256sum /usr/local/bin/xray | cut -d ' ' -f 1)" --client-version 26.10.10 --probe-url https://127.0.0.1:9443/ >/dev/null
nodectl takeover "$release" >/dev/null
nodectl apply "$release" > /root/nodectl-test/admin-node-apply.log
ip4=$(ip -4 -j address show dev eth0 | /integration-target/debug/repo-tools ipv4)
printf 'no-hosts\nport=1053\nlisten-address=127.0.0.1\nbind-interfaces\nno-resolv\nlocal=/integration.test/\naddress=/integration.test/%s\naddress=/integration.test/::1\nlocal=/localhost/\naddress=/localhost/%s\naddress=/localhost/::1\n' "$ip4" "$ip4" > /root/nodectl-test/admin-dns.conf
dnsmasq --keep-in-foreground --conf-file=/root/nodectl-test/admin-dns.conf > /root/nodectl-test/admin-dns.log 2>&1 &
printf '%s\n' "$!" > /root/nodectl-test/admin-dns.pid
mkdir -p /etc/systemd/system/nodectl-admin.service.d
printf '[Service]\nEnvironment=NODECTL_TEST_DNS=yes\n' > /etc/systemd/system/nodectl-admin.service.d/isolated-dns.conf
sha256sum /etc/dmit-node/node.toml /etc/dmit-node/secrets.json /etc/nginx/nginx.conf /etc/xray/config.json > /root/nodectl-test/admin-baseline.sha256
systemctl show nginx xray -p MainPID > /root/nodectl-test/admin-node-pids.before
nodectl admin install --assets /opt/nodectl-admin-test-ui --port "$port" --execute > /root/nodectl-test/admin-install.log
grep -E '^[0-9a-f]{64}$' /root/nodectl-test/admin-install.log > /root/nodectl-test/admin-token
test "$(wc -l < /root/nodectl-test/admin-token)" = 1
chmod 0600 /root/nodectl-test/admin-token
sha256sum --check /root/nodectl-test/admin-baseline.sha256 >/dev/null
systemctl show nginx xray -p MainPID > /root/nodectl-test/admin-node-pids.after
cmp /root/nodectl-test/admin-node-pids.before /root/nodectl-test/admin-node-pids.after
curl --noproxy '*' --fail --silent --show-error --cacert /usr/local/share/ca-certificates/nodectl-test.crt --resolve "node.localhost:$port:127.0.0.1" "https://node.localhost:$port/admin/" > /root/nodectl-test/admin-root.html
systemctl is-active --quiet nginx xray nodectl-share nodectl-admin nodectl-admin-nginx
printf '%s\n' ADMIN_HTTPS_AND_ORIGINAL_NODE_BYTES_AND_PIDS_PRESERVED_PASS
