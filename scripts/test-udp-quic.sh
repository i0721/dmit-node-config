#!/bin/bash
# Called while the isolated server is held at its network acceptance gate.
set -euo pipefail
server=${1:?isolated server container}
image=${2:?same distro client image}
work=${3:?private driver work directory}
umask 077
client=
cleanup() {
  result=$?
  if [ "$result" != 0 ] && [ "${NODECTL_KEEP_FAILED:-}" = yes ] && [ -n "$client" ]; then echo "Retained private client fixture: $client";
  else [ -z "$client" ] || docker rm -f "$client" >/dev/null 2>&1 || true; fi
}
trap cleanup EXIT
docker exec "$server" ip address replace 198.18.0.2/32 dev lo
docker exec -d "$server" /integration-target/debug/repo-tools udp-echo 198.18.0.2:14444
docker exec -d "$server" /integration-target/debug/repo-tools udp-dns 198.18.0.2:53
docker cp "$server:/root/nodectl-test/clients/." "$work/" >/dev/null
docker cp "$server:/usr/local/share/ca-certificates/nodectl-test.crt" "$work/ca.crt" >/dev/null
server_ip=$(docker inspect --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$server")
start_client() {
  engine=$1; kind=$2; negative=${3:-}
  client="nodectl-udp-$engine-$kind-$$"
  docker run --detach --name "$client" --label nodectl.isolated-test=true --cap-add NET_ADMIN --device /dev/net/tun --entrypoint /bin/sleep -v "$work:/acceptance:ro" "$image" infinity >/dev/null
  docker exec "$client" bash -lc 'cp /acceptance/ca.crt /usr/local/share/ca-certificates/nodectl-test.crt; update-ca-certificates >/dev/null 2>&1'
  config_args=(protocol-config /acceptance "$engine" "$kind" "$server_ip" /root/client)
  [ -z "$negative" ] || config_args+=("$negative")
  docker exec -e NODECTL_CAPTURE_HELLO=yes "$client" /integration-target/debug/repo-tools "${config_args[@]}"
  docker exec -d "$client" /integration-target/debug/repo-tools hello-relay /root/client
  docker exec "$client" /usr/local/bin/mihomo -t -d /root/mihomo-state -f /root/client/mihomo.yaml >/dev/null
  if [ "$engine" = xray ]; then
    docker exec "$client" /usr/local/bin/xray run -test -config /root/client/xray.json >/dev/null
    docker exec -d "$client" bash -lc '/usr/local/bin/xray run -config /root/client/xray.json > /root/xray-client.log 2>&1'
  fi
  docker exec -d "$client" bash -lc '/usr/local/bin/mihomo -d /root/mihomo-state -f /root/client/mihomo.yaml > /root/mihomo-client.log 2>&1'
  ready=no
  for _ in $(seq 1 30); do if docker exec "$client" ip link show verify0 >/dev/null 2>&1; then ready=yes; break; fi; sleep 1; done
  [ "$ready" = yes ] || { echo "TUN startup failed: $engine $kind" >&2; exit 1; }
  # Only this disposable client network namespace gets a new default route.
  docker exec "$client" bash -lc "gateway=\$(ip -4 route show default | awk '{print \$3}'); ip route add $server_ip/32 via \"\$gateway\" dev eth0; ip route replace default dev verify0; ip route get 198.18.0.2 | grep -q 'dev verify0'; ip route get $server_ip | grep -q 'dev eth0'"
}
web() {
  docker run --rm --network "container:$client" -v "$work/ca.crt:/ca.crt:ro" nodectl-integration-h3 --noproxy '*' --fail --silent --show-error --max-time 10 --cacert /ca.crt --resolve node.integration.test:443:198.18.0.2 "$@" https://node.integration.test/
}
for engine in xray mihomo; do
  for kind in xhttp-chrome ws reality; do
    [ -z "${NODECTL_PROTOCOL_ONLY:-}" ] || [ "$NODECTL_PROTOCOL_ONLY" = "$engine:$kind" ] || continue
    start_client "$engine" "$kind"
    web --socks5 127.0.0.1:10811 | grep -q integration-ready
    # SOCKS UDP accepts four independent DNS replies and varying-size echoes.
    port=10811; [ "$engine" != xray ] || port=10810
    docker exec "$client" /integration-target/debug/repo-tools udp-probe "$port" 198.18.0.2 53 dns
    docker exec "$client" /integration-target/debug/repo-tools udp-probe "$port" 198.18.0.2 14444
    version=$(web --http3-only --output /dev/null --write-out '%{http_version}')
    [ "$version" = 3 ] || { echo "QUIC failed: $engine $kind" >&2; exit 1; }
    docker exec "$client" /integration-target/debug/repo-tools hello-check /root/client "$kind"
    # No live transport remains: the same forced QUIC request must now fail.
    docker exec "$client" pkill -x mihomo
    if web --http3-only --output /dev/null >/dev/null 2>&1; then echo 'UDP direct leak detected' >&2; exit 1; fi
    echo "TCP_UDP_DNS_ECHO_QUIC_NO_LEAK_PASS:$engine:$kind"
    cleanup; client=
  done
done
if [ -n "${NODECTL_PROTOCOL_ONLY:-}" ]; then exit 0; fi
for engine in xray mihomo; do
 for kind in xhttp-chrome ws reality; do
  for negative in wrong-key wrong-flow missing-encryption legacy; do
    start_client "$engine" "$kind" "$negative"
    if web --socks5 127.0.0.1:10811 --output /dev/null >/dev/null 2>&1; then echo "Invalid client accepted: $kind $negative" >&2; exit 1; fi
    echo "ENCRYPTION_NEGATIVE_REFUSED_PASS:$engine:$kind:$negative"
    cleanup; client=
  done
 done
done
echo TWO_ENGINES_THREE_TRANSPORTS_UDP_QUIC_AND_NEGATIVES_PASS
