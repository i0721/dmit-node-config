#!/bin/bash
# Run at the fixture's network gate, before the protocol/fault suites resume.
set -euo pipefail
umask 077
work=$(mktemp -d /root/nodectl-sharing-wizard-XXXXXX)
dns_pid=
trap 'if [ -n "$dns_pid" ]; then kill "$dns_pid" 2>/dev/null || true; fi; rm -rf "$work"' EXIT
ip4=$(ip -4 -j address show dev eth0 | /integration-target/debug/repo-tools ipv4)
dnsmasq --no-daemon --conf-file=/dev/null --port=1053 --listen-address=127.0.0.1 --bind-interfaces --no-resolv --no-hosts --address="/.integration.test/$ip4" --address=/.integration.test/::1 > "$work/dns.log" 2>&1 &
dns_pid=$!
sleep 0.5
fixture_token=$(printf 'fedcba9876543210%.0s' $(seq 1 4))
printf '%s' "$fixture_token" > "$work/token"
original=$(readlink /var/lib/dmit-node/current)
original_token=$(sha256sum /var/lib/dmit-node/distribution/access.json)
wizard="env NODECTL_TEST_DNS=yes nodectl --state-dir '$work/state' deploy --wizard --distribution true --access-token-file '$work/token' --skip-dependencies --manual-certificates --takeover --probe-url https://127.0.0.1:9443/"
answers() { printf '\n\n\n\n\n\n\n\n\n\nfalse\nfalse\nmanual\n\n\n%s\n' "$1"; }
if answers no | script -q -e -c "$wizard" "$work/cancel.tty" >/dev/null; then exit 1; fi
test ! -e "$work/state/distribution/access.json"
test "$(readlink /var/lib/dmit-node/current)" = "$original"
answers yes | script -q -e -c "$wizard" "$work/success.tty" >/dev/null
test -f "$work/state/distribution/access.json"
test -d "$work/state/client-exports"
test "$(stat -c %a "$work/state/distribution/access.json")" = 600
status=$({ printf 'url = "https://node.integration.test/nodes/sub/mihomo?token=%s"\n' "$fixture_token"; } | curl --config - --silent --max-time 5 --resolve node.integration.test:443:127.0.0.1 --output "$work/provider" --write-out '%{http_code}')
test "$status" = 200
grep -q '"proxies"' "$work/provider"
nodectl --state-dir "$work/state" uninstall --execute > "$work/uninstall.log"
test "$(readlink /var/lib/dmit-node/current)" = "$original"
test "$(sha256sum /var/lib/dmit-node/distribution/access.json)" = "$original_token"
systemctl is-active --quiet nginx xray nodectl-share
echo SHARING_WIZARD_CUSTOM_TOKEN_CANCEL_DEPLOY_EXPORT_AND_FOREIGN_BASELINE_RESTORE_PASS
