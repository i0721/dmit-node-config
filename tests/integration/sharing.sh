#!/bin/bash
set -euo pipefail
umask 077
work=$(mktemp -d /root/nodectl-sharing-XXXXXX)
trap 'rm -rf "$work"' EXIT
token_file=/var/lib/dmit-node/distribution/access.json
read_token() { sed -n 's/^  "token": "\([a-f0-9]*\)",$/\1/p' "$token_file"; }
token=$(read_token)
test "${#token}" = 64
test "$(stat -c %a "$token_file")" = 600
test "$(stat -c %a /var/lib/dmit-node/distribution)" = 700
request() {
  local path=$1 auth=${2:-} result= retry
  for retry in $(seq 1 20); do
    result=$({ printf 'url = "https://node.integration.test%s"\n' "$path"; [ -z "$auth" ] || printf 'header = "Authorization: Bearer %s"\n' "$auth"; } | curl --config - --silent --show-error --max-time 5 --resolve node.integration.test:443:127.0.0.1 --dump-header "$work/headers" --output "$work/body" --write-out '%{http_code}' 2> "$work/curl-error")
    [ "$result" = 429 ] || break
    sleep 0.25
  done
  printf '%s' "$result"
}
test "$(request /nodes/api "$token")" = 200
grep -qi 'cache-control:.*no-store' "$work/headers"
grep -qi 'referrer-policy: no-referrer' "$work/headers"
if [ "${1:-}" = verify ]; then
  grep -q "$2" "$work/body"
  echo SHARING_ROLLBACK_FOLLOWS_SUCCESS_AND_RETAINS_CURRENT_TOKEN_PASS
  exit 0
fi
systemctl is-active --quiet nodectl-share
test "$(request /nodes/)" = 200
grep -q 'type="password"' "$work/body"
test "$(request /nodes/api)" = 401
test "$(request /nodes/api "$(printf 'f%.0s' $(seq 1 64))")" = 401
for format in xray mihomo; do
  test "$(request "/nodes/sub/$format?token=$token")" = 200
  if [ "$format" = xray ]; then expected=profiles.json; else expected=proxies.yaml; fi
  cmp "$work/body" "/root/nodectl-test/clients/$expected"
  ! grep -qE '"(privateKey|decryption|config_file)"' "$work/body"
done
test "$(request "/nodes/sub/xray?token=$token&token=$token")" = 400
# Headers are parsed before a location is chosen; these credentials must not
# appear in the parent server error log either.
{ printf 'url = "https://node.integration.test/nodes/sub/xray?token=%s"\n' "$token"; printf 'header = "X-Large: %s"\n' "$(printf 'x%.0s' $(seq 1 70000))"; } | curl --config - --silent --max-time 5 --resolve node.integration.test:443:127.0.0.1 --output /dev/null > /dev/null 2> "$work/oversized-error" || true
{ printf 'url = "https://node.integration.test/nodes/sub/xray?token=%s&padding=%s"\n' "$token" "$(printf 'x%.0s' $(seq 1 70000))"; } | curl --config - --silent --max-time 5 --resolve node.integration.test:443:127.0.0.1 --output /dev/null > /dev/null 2> "$work/oversized-uri-error" || true
{ printf 'url = "https://node.integration.test/?token=%s"\n' "$token"; printf 'header = "Referer: https://node.integration.test/nodes/sub/xray?token=%s"\n' "$token"; } | curl --config - --silent --max-time 5 --resolve node.integration.test:443:127.0.0.1 --output /dev/null > /dev/null 2> "$work/fallback-log-error" || true
nodectl share rotate > "$work/rotate-output"
test "$(request /nodes/api "$token")" = 401
old=$token
token=$(read_token)
test "$token" != "$old"
test "$(request /nodes/api "$token")" = 200
nodectl share revoke > "$work/revoke-output"
test "$(request /nodes/api "$token")" = 401
nodectl share rotate > "$work/restore-output"
for secret in "$old" "$token"; do
  ! grep -RFl -- "$secret" /var/log/nginx "$work/rotate-output" "$work/revoke-output" "$work/restore-output" > /dev/null
done
for attempt in $(seq 1 25); do
  curl --silent --max-time 5 --resolve node.integration.test:443:127.0.0.1 --header "X-Forwarded-For: 192.0.2.$attempt" --output /dev/null --write-out '%{http_code}' https://node.integration.test/nodes/ > "$work/rate-$attempt" &
done
wait
grep -ql 429 "$work"/rate-*
echo SHARING_PER_IP_RATE_AND_CONNECTION_LIMIT_WITH_SPOOFED_HEADERS_PASS
echo SHARING_HTTPS_AUTH_FORMATS_ROTATION_REVOCATION_NO_LOG_LEAK_PASS
