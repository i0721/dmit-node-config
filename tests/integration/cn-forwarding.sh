#!/bin/bash
# Real kernel sockets and locked Xray, simulated TUN peer; no WARP registration.
set -euo pipefail
if [ "${1:-}" != --inside ]; then
  exec unshare --mount --net --fork bash "$0" --inside "$(realpath "${1:?nodectl}")" "$(realpath "${2:?repo-tools}")" "$(realpath "${3:?unit-test executable}")" "$(realpath "${4:?locked Xray}")" "$(realpath "${5:?locked geodata directory}")"
fi
binary=$2; tools=$3; tests=$4; xray=$5; assets=$6
mount --make-rprivate /
mount -t tmpfs tmpfs /run
mount -t sysfs sysfs /sys
ip link set lo up
printf '1\n' > /proc/sys/net/ipv4/conf/all/rp_filter
work=$(mktemp -d /var/tmp/nodectl-cn-packets-XXXXXX)
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  for pid in $(ip netns pids peer 2>/dev/null); do kill "$pid" 2>/dev/null || true; done
  ip netns del peer 2>/dev/null || true
  rm -rf -- "$work"
}
trap cleanup EXIT
chmod 0755 "$work"
cp "$xray" "$work/xray"
cp "$assets/geoip.dat" "$assets/geosite.dat" "$work/"
chmod 0755 "$work/xray"
chmod 0644 "$work/"*.dat
CN_ROUTING_TEST_CONFIG="$work/xray.json" "$tests" --exact service_tests::generate_cn_forwarding_fixture_from_production_model --ignored >/dev/null
chmod 0644 "$work/xray.json"
ip netns add peer
ip -n peer link set lo up
args=(--interface warp-cn --table 5271 --priority 15271 --mark 5271)
"$binary" cn-route prepare "${args[@]}"
for pair in 'native0 1' 'warp-cn 2'; do
  set -- $pair
  ip link add "$1" type veth peer name "peer$2"
  ip link set "peer$2" netns peer
  ip addr add "10.203.$2.1/24" dev "$1"
  ip -6 addr add "fd11:$2::1/64" dev "$1" nodad
  ip link set "$1" up
  ip -n peer addr add "10.203.$2.2/24" dev "peer$2"
  ip -n peer -6 addr add "fd11:$2::2/64" dev "peer$2" nodad
  ip -n peer link set "peer$2" up
done
ip route add default via 10.203.1.2 dev native0
ip -6 route add default via fd11:1::2 dev native0
for address in 114.114.114.114 8.8.8.8 223.5.5.5 1.1.1.1 203.0.113.1; do ip -n peer addr add "$address/32" dev lo; done
ip -n peer -6 addr add 240e::114/128 dev peer2 nodad
ip -n peer -6 addr add 2606:4700:4700::1111/128 dev peer1 nodad
ip netns exec peer "$tools" tcp-peer-marker 0.0.0.0:18080 & pids+=("$!")
ip netns exec peer "$tools" tcp-peer-marker '[::]:18081' & pids+=("$!")
ip netns exec peer "$tools" udp-echo 0.0.0.0:14444 & pids+=("$!")
ip netns exec peer "$tools" udp-dns 223.5.5.5:53 & dns_cn=$!; pids+=("$dns_cn")
ip netns exec peer "$tools" udp-dns 1.1.1.1:53 & pids+=("$!")
"$binary" cn-route ready "${args[@]}"
test "$(cat /proc/sys/net/ipv4/conf/all/rp_filter)" = 1
test "$(cat /proc/sys/net/ipv4/conf/warp-cn/rp_filter)" = 2
tc qdisc add dev warp-cn clsact
tc filter add dev warp-cn egress protocol ip pref 10 handle 5271 fw action pass
setpriv --reuid=65534 --regid=65534 --clear-groups --inh-caps=+net_raw --ambient-caps=+net_raw --bounding-set=-all,+net_raw env XRAY_LOCATION_ASSET="$work" "$work/xray" run -config "$work/xray.json" >/dev/null 2>&1 & pids+=("$!")
web() { curl --disable --noproxy '' --socks5-hostname 127.0.0.1:10810 -fsS --max-time 5 "$1"; }
for _ in $(seq 1 30); do if web http://8.8.8.8:18080/ >/dev/null 2>&1; then break; fi; sleep 0.1; done
test "$(web http://114.114.114.114:18080/)" = 10.203.2.1
tc -s filter show dev warp-cn egress
test "$(web http://8.8.8.8:18080/)" = 10.203.1.1
test "$(web 'http://[240e::114]:18081/')" = fd11:2::1
test "$(web 'http://[2606:4700:4700::1111]:18081/')" = fd11:1::1
test "$(web http://www.baidu.com:18080/)" = 10.203.2.1
test "$(web http://example.net:18080/)" = 10.203.1.1
"$tools" udp-probe 10810 114.114.114.114 14444
"$tools" udp-probe 10810 8.8.8.8 14444
# Count only forbidden native packets; total NIC counters also include normal
# IPv6 discovery and final ACKs from preceding successful direct connections.
tc qdisc add dev native0 clsact
tc filter add dev native0 egress protocol ip pref 10 flower dst_ip 114.114.114.114 action pass
tc filter add dev native0 egress protocol ip pref 11 flower ip_proto udp dst_port 53 action pass
tc filter add dev native0 egress protocol ipv6 pref 12 flower dst_ip 240e::114 action pass
no_leak() {
  test "$(tc -s filter show dev native0 egress | sed -n 's/.*Sent \([0-9]*\) bytes.*/\1/p')" = "$(printf '0\n0\n0')"
}
# No CN DNS query may fall back through the native interface.
kill "$dns_cn"
if web http://uncached-check.baidu.com:18080/ >/dev/null 2>&1; then echo 'CN DNS unexpectedly fell back' >&2; exit 1; fi
sleep 12 # Include late resolver retries after the client deadline.
no_leak
"$binary" cn-route block "${args[@]}"
if reply=$(web http://114.114.114.114:18080/ 2>/dev/null); then
  echo "Blocked CN path unexpectedly reached peer from $reply" >&2
  ip route show table 5271 >&2
  ip rule show >&2
  ip route get 114.114.114.114 mark 5271 oif warp-cn >&2 || true
  ip route get 114.114.114.114 mark 5271 >&2 || true
  tc -s filter show dev warp-cn egress >&2
  exit 1
fi
no_leak
test "$(web http://8.8.8.8:18080/)" = 10.203.1.1
ip link set warp-cn up
"$binary" cn-route ready "${args[@]}"
ip link del warp-cn
if web http://114.114.114.114:18080/ >/dev/null 2>&1; then exit 1; fi
no_leak
test "$(web http://8.8.8.8:18080/)" = 10.203.1.1
ip route show default | grep -q 'via 10.203.1.2 dev native0'
echo CN_SIMULATED_TUN_GEOSITE_GEOIP_IPV4_IPV6_TCP_UDP_MINIMAL_CAP_DNS_FAILURE_BLOCK_DELETE_NO_NATIVE_LEAK_PASS
