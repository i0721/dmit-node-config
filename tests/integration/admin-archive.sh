#!/bin/bash
# Explicit disposable fixture only; exercises the production static installer.
set -euo pipefail
umask 077
test "${NODECTL_ADMIN_TEST:-}" = isolated
port=${NODECTL_ADMIN_TEST_PORT:-10443}
[[ "$port" =~ ^[0-9]+$ ]] && [ "$port" -ge 1024 ] && [ "$port" -lt 65535 ] || exit 1
blocked=$((port + 1))
test -f /root/nodectl-test/admin-token
archive=/root/nodectl-test/admin-static.tar.gz
test -s "$archive"
install -m 0755 /integration-target/debug/nodectl /usr/local/bin/.nodectl-admin-next
mv -f /usr/local/bin/.nodectl-admin-next /usr/local/bin/nodectl
sha256sum /etc/dmit-node/node.toml /etc/dmit-node/secrets.json /etc/nginx/nginx.conf /etc/xray/config.json /var/lib/dmit-node/admin/credential.json > /root/nodectl-test/archive-node-before.sha256
systemctl show nginx xray -p MainPID > /root/nodectl-test/archive-pids.before
nodectl admin install --archive "$archive" --port "$port" --execute > /root/nodectl-test/archive-install.log 2>&1
sha256sum --check --status /root/nodectl-test/archive-node-before.sha256
systemctl show nginx xray -p MainPID > /root/nodectl-test/archive-pids.after
cmp /root/nodectl-test/archive-pids.before /root/nodectl-test/archive-pids.after
test -s /usr/local/share/nodectl-admin/current/admin/THIRD_PARTY.txt
sha256sum /etc/nodectl-admin/nginx.conf /etc/systemd/system/nodectl-admin.service /etc/systemd/system/nodectl-admin-nginx.service > /root/nodectl-test/archive-admin-before.sha256
readlink /usr/local/share/nodectl-admin/current > /root/nodectl-test/archive-static.before
systemctl is-enabled nodectl-admin nodectl-admin-nginx > /root/nodectl-test/archive-enabled.before
systemd-socket-activate -l "$blocked" /bin/cat > /root/nodectl-test/archive-port.log 2>&1 &
listener=$!
trap 'kill "$listener" 2>/dev/null || true' EXIT
sleep 1
kill -0 "$listener"
if nodectl admin install --archive "$archive" --port "$blocked" --execute > /root/nodectl-test/archive-rejected.log 2>&1; then
  echo 'Occupied port must fail installation' >&2; exit 1
fi
sha256sum --check --status /root/nodectl-test/archive-node-before.sha256
sha256sum --check --status /root/nodectl-test/archive-admin-before.sha256
readlink /usr/local/share/nodectl-admin/current > /root/nodectl-test/archive-static.after
cmp /root/nodectl-test/archive-static.before /root/nodectl-test/archive-static.after
systemctl is-enabled nodectl-admin nodectl-admin-nginx > /root/nodectl-test/archive-enabled.after
cmp /root/nodectl-test/archive-enabled.before /root/nodectl-test/archive-enabled.after
systemctl is-active --quiet nginx xray nodectl-admin nodectl-admin-nginx
systemctl show nginx xray -p MainPID > /root/nodectl-test/archive-pids.after
cmp /root/nodectl-test/archive-pids.before /root/nodectl-test/archive-pids.after
curl --noproxy '*' --fail --silent --show-error --cacert /usr/local/share/ca-certificates/nodectl-test.crt --resolve "node.localhost:$port:127.0.0.1" "https://node.localhost:$port/admin/" >/dev/null
printf '%s\n' ADMIN_STATIC_ARCHIVE_INSTALL_AND_FAILED_REINSTALL_RESTORE_PASS
