#!/bin/bash
# Reproducible real-API/browser acceptance. Requires Docker, pnpm and Chromium.
set -euo pipefail
cd "$(dirname "$0")/.."
port=${NODECTL_ADMIN_TEST_PORT:-10443}
[[ "$port" =~ ^[0-9]+$ ]] && [ "$port" -ge 1024 ] && [ "$port" -lt 65535 ] || exit 1
name="nodectl-admin-e2e-$$"
mkdir -p dist
log=dist/admin-runtime.log
cleanup() {
  result=$?
  if [ "$result" != 0 ] && [ "${NODECTL_KEEP_FAILED:-}" = yes ]; then
    printf 'Retained isolated admin fixture: %s\n' "$name"
  else docker rm -f "$name" >/dev/null 2>&1 || true; fi
}
trap cleanup EXIT
pnpm -C webui build
pnpm -C webui performance:check
pnpm -C webui licenses list --prod --json > dist/admin-frontend-licenses.json
node scripts/package-webui.mjs
tar -C dist/admin-static -czf dist/admin-test-static.tar.gz webui webui-manifest.json
docker build --build-arg BASE=debian:12 -f tests/integration/Dockerfile -t nodectl-admin-integration . > dist/admin-runtime-build.log 2>&1
docker run --detach --tty --name "$name" --label nodectl.isolated-test=true --privileged --cgroupns=host --env container=docker --tmpfs /run --tmpfs /run/lock --tmpfs /tmp -v /sys/fs/cgroup:/sys/fs/cgroup:rw -p "127.0.0.1:$port:$port" nodectl-admin-integration >/dev/null
ready=no
for _ in $(seq 1 60); do
  if docker exec "$name" systemctl show-environment >/dev/null 2>&1; then ready=yes; break; fi
  sleep 1
done
[ "$ready" = yes ] || { echo 'systemd did not boot' >&2; exit 1; }
docker exec "$name" mkdir -p /opt/nodectl-admin-test-ui
docker cp webui/apps/web/dist/. "$name:/opt/nodectl-admin-test-ui/"
docker cp dist/admin-test-static.tar.gz "$name:/root/admin-static.tar.gz"
docker exec -e NODECTL_ADMIN_TEST=isolated -e NODECTL_ADMIN_TEST_PORT="$port" "$name" bash /work/tests/integration/admin-runtime.sh > "$log" 2>&1
docker exec "$name" mv /root/admin-static.tar.gz /root/nodectl-test/admin-static.tar.gz
docker exec -e NODECTL_ADMIN_TEST=isolated -e NODECTL_ADMIN_TEST_PORT="$port" "$name" bash /work/tests/integration/admin-archive.sh >> "$log" 2>&1
export NODECTL_ADMIN_TEST_CONTAINER="$name"
export NODECTL_ADMIN_TEST_URL="https://node.localhost:$port"
pnpm -C webui check
printf '%s\n' ADMIN_FULL_ACCEPTANCE_PASS
