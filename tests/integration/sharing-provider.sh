#!/bin/bash
# Actual Mihomo HTTP-provider import followed by a real proxied HTTPS request.
set -euo pipefail
umask 077
work=$(mktemp -d /root/nodectl-sharing-provider-XXXXXX)
client=
trap 'if [ -n "$client" ]; then kill "$client" 2>/dev/null || true; fi; rm -rf "$work"' EXIT
token=$(sed -n 's/^  "token": "\([a-f0-9]*\)",$/\1/p' /var/lib/dmit-node/distribution/access.json)
test "${#token}" = 64
cat > "$work/config.json" <<JSON
{
  "socks-port": 6099, "allow-lan": false, "bind-address": "127.0.0.1", "log-level": "silent",
  "hosts": {"node.integration.test":"127.0.0.1", "probe.integration.test":"127.0.0.1"},
  "proxy-providers": {"downloaded": {"type":"http", "url":"https://node.integration.test/nodes/sub/mihomo?token=$token", "proxy":"DIRECT", "path":"./provider.yaml", "interval":3600}},
  "proxy-groups": [{"name":"SUBSCRIPTION", "type":"select", "use":["downloaded"]}],
  "rules": ["MATCH,SUBSCRIPTION"]
}
JSON
/usr/local/bin/mihomo -d "$work" -f "$work/config.json" > "$work/client.log" 2>&1 &
client=$!
passed=no
for _ in $(seq 1 30); do
  if [ -s "$work/provider.yaml" ] && curl --fail --silent --max-time 3 --socks5-hostname 127.0.0.1:6099 https://probe.integration.test:9443/ > "$work/response" 2> "$work/curl.log"; then passed=yes; break; fi
  kill -0 "$client" 2>/dev/null || break
  sleep 0.2
done
test "$passed" = yes
cmp "$work/provider.yaml" /root/nodectl-test/clients/proxies.yaml
echo SHARING_REAL_MIHOMO_HTTP_PROVIDER_IMPORT_AND_HTTPS_HANDSHAKE_PASS
