#!/bin/bash
# Explicit disposable network fixture only. Reuses two authorized credentials;
# never registers devices, changes main routes, or accepts terms implicitly.
set -euo pipefail
test "${NODECTL_WARP_REAL_TEST:-}" = isolated-authorized
work=${1:?root-private fixture directory}
test "$work" = /root/warp-real
umask 077
cd "$work"
for name in old cn; do test "$(stat -c %a "$name.json")" = 600; done
test -f main-before.json
cmp main-before.json <(ip -j route show table main)
args=(--interface warp-cn --table 5271 --priority 15271 --mark 5271)
./nodectl cn-route ready "${args[@]}"
tc qdisc add dev eth0 clsact
tc filter add dev eth0 egress protocol ip pref 10 flower dst_ip 223.5.5.5 action pass
tc filter add dev eth0 egress protocol ip pref 11 flower dst_ip 114.114.114.114 action pass
tc filter add dev eth0 egress protocol ipv6 pref 12 flower dst_ip 2400:3200::1 action pass
no_leak() {
  test "$(tc -s filter show dev eth0 egress | sed -n 's/.*Sent \([0-9]*\) bytes.*/\1/p')" = "$(printf '0\n0\n0')"
}
web() { curl --disable --noproxy '' --socks5-hostname 127.0.0.1:10810 -fsS --max-time 10 "$1"; }
for iface in tun0 warp-cn; do
  curl --noproxy '*' -fsS --max-time 15 --interface "$iface" https://1.1.1.1/cdn-cgi/trace > "$iface-trace.txt"
  grep -Eq '^warp=(on|plus)$' "$iface-trace.txt"
  echo "REAL_WARP_TRACE_PASS:$iface"
done
./repo-tools udp-probe 10810 223.5.5.5 53 dns
echo REAL_CN_IPV4_UDP_DNS_PASS
./repo-tools udp-probe 10810 2400:3200::1 53 dns
echo REAL_CN_IPV6_UDP_DNS_PASS
web https://www.baidu.com/ > baidu.html
echo REAL_CN_DOMAIN_HTTPS_PASS
web https://1.1.1.1/cdn-cgi/trace > native-trace.txt
grep -q '^warp=off$' native-trace.txt
echo REAL_NON_CN_NATIVE_TRACE_PASS
no_leak
# Only terminate the explicitly authorized CN process in this disposable fixture.
found=false
for pid in $(pgrep -x usque); do
  if tr '\0' '\n' < "/proc/$pid/cmdline" | grep -qx "$work/cn.json"; then
    kill -TERM "$pid"
    found=true
  fi
done
test "$found" = true
./nodectl cn-route block "${args[@]}"
if web https://114.114.114.114/ > blocked.txt 2>/dev/null; then exit 1; fi
if web https://uncached-isolated-check.baidu.com/ > blocked-dns.txt 2>/dev/null; then exit 1; fi
sleep 12 # Include outstanding DNS retries after the client deadline.
no_leak
curl --noproxy '*' -fsS --max-time 15 --interface tun0 https://1.1.1.1/cdn-cgi/trace > old-after-stop.txt
grep -Eq '^warp=(on|plus)$' old-after-stop.txt
web https://1.1.1.1/cdn-cgi/trace > native-after-stop.txt
grep -q '^warp=off$' native-after-stop.txt
cmp main-before.json <(ip -j route show table main)
echo REAL_CN_STOP_NO_NATIVE_LEAK_OLD_WARP_AND_NATIVE_SURVIVE_PASS
# usque normally removes its own device on exit; exercise missing-device state.
if ip link show warp-cn >/dev/null 2>&1; then ip link del warp-cn; fi
if web https://114.114.114.114/ > deleted.txt 2>/dev/null; then exit 1; fi
no_leak
cmp main-before.json <(ip -j route show table main)
echo REAL_DUAL_WARP_IPV4_IPV6_DNS_CN_HTTPS_STOP_DELETE_NO_LEAK_PASS
