#!/bin/bash
set -euo pipefail
umask 077
test "${NODECTL_ADMIN_TEST:-}" = isolated
test -f /root/nodectl-test/admin-token
install -m 0755 /integration-target/debug/nodectl /usr/local/bin/.nodectl-admin-next
mv -f /usr/local/bin/.nodectl-admin-next /usr/local/bin/nodectl
cd /work
bash tests/integration/admin-localhost.sh
nodectl renew-hook >/dev/null
next=$(nodectl render)
nodectl check "$next" --client /usr/local/bin/xray --client-sha256 "$(sha256sum /usr/local/bin/xray | cut -d ' ' -f 1)" --client-version 26.10.10 --probe-url https://127.0.0.1:9443/ >/dev/null
nodectl apply "$next" >/dev/null
ip4=$(ip -4 -j address show dev eth0 | /integration-target/debug/repo-tools ipv4)
printf 'no-hosts\nport=1053\nlisten-address=127.0.0.1\nbind-interfaces\nno-resolv\nlocal=/integration.test/\naddress=/integration.test/%s\naddress=/integration.test/::1\nlocal=/localhost/\naddress=/localhost/%s\naddress=/localhost/::1\n' "$ip4" "$ip4" > /root/nodectl-test/admin-dns.conf
old_pid=$(cat /root/nodectl-test/admin-dns.pid)
if [ -r "/proc/$old_pid/comm" ] && [ "$(cat "/proc/$old_pid/comm")" = dnsmasq ]; then kill "$old_pid"; fi
dnsmasq --keep-in-foreground --conf-file=/root/nodectl-test/admin-dns.conf > /root/nodectl-test/admin-dns.log 2>&1 &
printf '%s\n' "$!" > /root/nodectl-test/admin-dns.pid
nodectl admin install --assets /opt/nodectl-admin-test-ui --port 10443 --execute > /root/nodectl-test/admin-reinstall.log
curl --noproxy '*' --fail --silent --show-error --cacert /usr/local/share/ca-certificates/nodectl-test.crt --resolve node.localhost:10443:127.0.0.1 https://node.localhost:10443/admin/ >/dev/null
printf '%s\n' ADMIN_LOCALHOST_TLS_SAN_AND_NODE_APPLY_PASS
