#!/bin/bash
# Explicitly authorized WSL-only test driver. Never invokes SSH or public DNS/
# certificate APIs, registers a device, or runs a deployment on the WSL host.
set -euo pipefail
test "${1:-}" = --authorized-local-test
test "$(id -u)" = 0
test -n "${WSL_DISTRO_NAME:-}"
repo=/mnt/d/coder/i721/dmit-node-config
base=/root/nodectl-cnweb-test
native=$base/native-debian12
name=nodectl-cnweb-cn-systemd
scenario=cn-systemd-real.sh
case "${2:-}" in
  '') :;;
  --legacy-rollback) name=nodectl-cnweb-legacy-cn-rollback; scenario=legacy-cn-rollback.sh;;
  *) exit 1;;
esac
umask 077
cd "$native"
sha256sum --check SHA256SUMS > /dev/null
sha256sum --check "$base/cn-systemd-review.sha256" > /dev/null
test "$(./nodectl --version)" = 'nodectl 0.5.1'
if [ "$scenario" = legacy-cn-rollback.sh ]; then test "$(./nodectl-040 --version)" = 'nodectl 0.4.0'; fi
for profile in old cn; do
  test "$(stat -c %a "$base/warp-real/$profile.json")" = 600
done
docker image inspect nodectl-integration:debian-12 > /dev/null
test "$(docker image inspect -f '{{.Id}}' nodectl-integration:debian-12)" = "$(cat IMAGE_ID)"
if docker inspect "$name" > /dev/null 2>&1; then
  echo 'Refusing an existing fixture; preserve its data and inspect it first.' >&2
  exit 1
fi
logs=$(mktemp -d "$base/cn-systemd-private-XXXXXX")
docker create --tty --name "$name" --label nodectl.isolated-test=true --label nodectl.task=cnweb-real-systemd --privileged --cgroupns=host --env container=docker --tmpfs /run --tmpfs /run/lock --tmpfs /tmp -v /sys/fs/cgroup:/sys/fs/cgroup:rw nodectl-integration:debian-12 > "$logs/container-id"
test "$(docker inspect -f '{{index .Config.Labels "nodectl.task"}}' "$name")" = cnweb-real-systemd
name=$(cat "$logs/container-id")
cleanup() {
  status=$?
  trap - EXIT
  mkdir -p "$logs/fixture"
  docker cp "$name:/root/nodectl-test/." "$logs/fixture/" > /dev/null 2>&1 || true
  docker stop -t 5 "$name" > /dev/null 2>&1 || true
  chmod -R go-rwx "$logs"
  printf 'Private test logs: %s\n' "$logs"
  exit "$status"
}
trap cleanup EXIT
docker start "$name" > /dev/null
docker exec "$name" mkdir -p /integration-target/debug /work/tests/integration
docker cp "$native/nodectl" "$name:/integration-target/debug/nodectl"
docker cp "$native/repo-tools" "$name:/integration-target/debug/repo-tools"
for script in prepare.sh "$scenario"; do
  sed 's/\r$//' "$repo/tests/integration/$script" > "$logs/$script"
  docker cp "$logs/$script" "$name:/work/tests/integration/$script"
done
if [ "$scenario" = legacy-cn-rollback.sh ]; then docker cp "$native/nodectl-040" "$name:/root/nodectl-040"; fi
docker exec "$name" bash /work/tests/integration/prepare.sh > "$logs/prepare.log" 2>&1
docker exec "$name" install -d -m 700 /etc/usque /etc/usque-cn /var/lib/nodectl-geodata/202610172205
docker cp "$base/warp-real/old.json" "$name:/etc/usque/config.json"
docker cp "$base/warp-real/cn.json" "$name:/etc/usque-cn/config.json"
docker cp "$base/usque" "$name:/usr/local/bin/usque"
docker cp "$base/speedtest-backend" "$name:/usr/local/bin/speedtest-backend"
docker cp "$base/openlist" "$name:/usr/local/bin/openlist"
for asset in geoip.dat geosite.dat; do docker cp "$base/assets/$asset" "$name:/var/lib/nodectl-geodata/202610172205/$asset"; done
docker exec "$name" bash -c 'chmod 755 /usr/local/bin/usque /usr/local/bin/speedtest-backend /usr/local/bin/openlist; chmod 600 /etc/usque/config.json /etc/usque-cn/config.json; chmod 644 /var/lib/nodectl-geodata/202610172205/*.dat'
docker exec -d "$name" bash -c 'exec /integration-target/debug/repo-tools probe > /root/nodectl-test/probe.log 2>&1'
docker exec -e NODECTL_REAL_WARP_TEST=authorized-two-existing-devices -e NODECTL_CN_TEST_SCENARIO="$scenario" "$name" bash -c '
set -euo pipefail
umask 077
ip4=$(ip -4 -j address show dev eth0 | /integration-target/debug/repo-tools ipv4)
dnsmasq --no-daemon --conf-file=/dev/null --port=1053 --listen-address=127.0.0.1 --bind-interfaces --no-resolv --no-hosts --address="/.integration.test/$ip4" --address=/.integration.test/::1 > /root/nodectl-test/dns.log 2>&1 &
dns_pid=$!
trap "kill $dns_pid 2>/dev/null || true" EXIT
sleep 0.3
export NODECTL_TEST_DNS=yes
timeout --signal=TERM --kill-after=10 900 bash "/work/tests/integration/$NODECTL_CN_TEST_SCENARIO"
' > "$logs/result.log" 2>&1
grep -E '^REAL_CN_SYSTEMD_|^LEGACY_CN_|^UDP_DNS_' "$logs/result.log"
echo REAL_CN_SYSTEMD_LOCAL_FIXTURE_COMPLETED
