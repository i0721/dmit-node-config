#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
base=${1:-debian:12}
case "$base" in debian:12|debian:13|ubuntu:24.04|ubuntu:26.04) :;; *) echo 'Unsupported test image' >&2; exit 1;; esac
label=${base//[:.]/-}
name="nodectl-test-$label-$$"
mkdir -p dist
log="dist/integration-$label-$(uname -m).log"
build_log="dist/integration-build-$label-$(uname -m).log"
work=$(mktemp -d)
cleanup() {
  result=$?
  if [ "$result" != 0 ] && [ "${NODECTL_KEEP_FAILED:-}" = yes ]; then echo "Retained isolated failed test container: $name";
  else docker rm -f "$name" >/dev/null 2>&1 || true; fi
  rm -rf "$work"
}
trap cleanup EXIT
echo "Building clean $base integration environment..."
proxy_args=()
for key in HTTP_PROXY HTTPS_PROXY NO_PROXY; do [ -z "${!key:-}" ] || proxy_args+=(--build-arg "$key"); done
docker build --network host "${proxy_args[@]}" --build-arg BASE="$base" -f tests/integration/Dockerfile -t "nodectl-integration:$label" . > "$build_log" 2>&1 || { tail -50 "$build_log"; exit 1; }
docker build --network host "${proxy_args[@]}" -f tests/integration/Dockerfile.h3 -t nodectl-integration-h3 . >> "$build_log" 2>&1 || { tail -50 "$build_log"; exit 1; }
docker run --detach --tty --name "$name" --label nodectl.isolated-test=true --privileged --cgroupns=host --env container=docker --tmpfs /run --tmpfs /run/lock --tmpfs /tmp -v /sys/fs/cgroup:/sys/fs/cgroup:rw "nodectl-integration:$label" >/dev/null
ready=no
for _ in $(seq 1 60); do
  if docker exec "$name" systemctl show-environment >/dev/null 2>&1; then ready=yes; break; fi
  sleep 1
done
[ "$ready" = yes ] || { docker logs "$name"; echo 'systemd did not boot' >&2; exit 1; }
: > "$log"
docker exec -e NODECTL_NETWORK_GATE=yes "$name" bash /work/tests/integration/runtime.sh >> "$log" 2>&1 &
runtime=$!
network_ready=no
for _ in $(seq 1 120); do
  if docker exec "$name" test -f /root/nodectl-test/network-ready; then network_ready=yes; break; fi
  kill -0 "$runtime" 2>/dev/null || break
  sleep 1
done
if [ "$network_ready" != yes ]; then cat "$log"; echo 'Runtime did not reach network gate' >&2; exit 1; fi
docker cp "$name:/usr/local/share/ca-certificates/nodectl-test.crt" "$work/ca.crt" >/dev/null
ip4=$(docker inspect --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$name")
version=$(docker run --rm -v "$work/ca.crt:/ca.crt:ro" nodectl-integration-h3 --fail --silent --show-error --max-time 20 --cacert /ca.crt --resolve "node.integration.test:443:$ip4" --http3-only --output /dev/null --write-out '%{http_version}' https://node.integration.test/)
[ "$version" = 3 ] || { echo 'External namespace HTTP3 failed' >&2; exit 1; }
echo 'EXTERNAL_NAMESPACE_HTTP3_PASS' >> "$log"
docker exec "$name" touch /root/nodectl-test/network-verified
set +e
wait "$runtime"
result=$?
set -e
cat "$log"
exit "$result"
