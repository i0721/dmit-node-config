#!/bin/bash
# Only after separate authorization: fresh disposable systemd fixture, two
# existing independently registered profiles, no registration or public DNS/CA.
set -euo pipefail
test -f /.dockerenv
test "${NODECTL_REAL_WARP_TEST:-}" = authorized-two-existing-devices
test "$(nodectl --version)" = 'nodectl 0.5.1'
test ! -e /var/lib/dmit-node/current
test ! -e /run/nodectl-cn-policy/owner.json
umask 077
work=$(mktemp -d /root/nodectl-cn-systemd-XXXXXX)
for profile in /etc/usque/config.json /etc/usque-cn/config.json; do
  test "$(stat -c %a "$profile")" = 600
done
credentials=$(sha256sum /etc/usque/config.json /etc/usque-cn/config.json)
ip -j route show table main > "$work/main4.json"
ip -6 -j route show table main default > "$work/default6.json"
# Address assignment creates kernel-owned host/link-local routes for the two
# new TUNs. Preserve every other main-table route and all default routes.
main6() { ip -6 route show table main | sed -E '/ dev (tun0|warp-cn) proto kernel /d'; }
main6 > "$work/main6.txt"
original=$(sha256sum /etc/nginx/nginx.conf /etc/dmit-node/secrets.json)
intact() {
  cmp "$work/main4.json" <(ip -j route show table main)
  cmp "$work/default6.json" <(ip -6 -j route show table main default)
  cmp "$work/main6.txt" <(main6)
  test "$credentials" = "$(sha256sum /etc/usque/config.json /etc/usque-cn/config.json)"
  test ! -e /var/lib/dmit-node/deployment.json
  test ! -e /var/lib/dmit-node/transaction.json
  test ! -e /var/lib/nodectl-web-guard/active
}
args=(--non-interactive --skip-dependencies --manual-certificates --takeover --warp true --cn-warp true --web-services true --probe-url https://127.0.0.1:9443/)
preview() {
  nodectl deploy "${args[@]}" > "$work/preview.txt"
  approval=$(sed -n 's/^Plan SHA256: //p' "$work/preview.txt")
  test "${#approval}" = 64
}
preview
if NODECTL_FAILPOINT=cn-warp nodectl deploy "${args[@]}" --execute --approve "$approval" > "$work/failure.txt" 2>&1; then exit 1; fi
grep -q 'Injected integration failure: cn-warp' "$work/failure.txt"
intact
test "$original" = "$(sha256sum /etc/nginx/nginx.conf /etc/dmit-node/secrets.json)"
curl --noproxy '*' -fsS http://127.0.0.1:9081/ | grep -q legacy-service
echo REAL_CN_SYSTEMD_FAILURE_RESTORES_ORIGINAL_HOST_PASS
preview
nodectl deploy "${args[@]}" --execute --approve "$approval" > "$work/success.txt" 2>&1
intact
systemctl is-active --quiet usque-warp usque-cn xray nginx openlist librespeed
nodectl doctor --json > "$work/doctor.json"
nodectl export-clients --format all --output "$work/clients" >/dev/null
/integration-target/debug/repo-tools protocol-config "$work/clients" xray xhttp-chrome 127.0.0.1 "$work/client"
/usr/local/bin/xray run -config "$work/client/xray.json" > "$work/client.log" 2>&1 &
client=$!
trap 'kill "$client" 2>/dev/null || true' EXIT
sleep 0.5
/integration-target/debug/repo-tools udp-probe 10810 223.5.5.5 53 dns
/integration-target/debug/repo-tools udp-probe 10810 2400:3200::1 53 dns
web() { curl --noproxy '' --socks5-hostname 127.0.0.1:10810 -fsS --max-time 15 "$1"; }
web https://www.baidu.com/ > "$work/cn.html"
web https://1.1.1.1/cdn-cgi/trace > "$work/native.txt"
grep -q '^warp=off$' "$work/native.txt"
echo REAL_CN_SYSTEMD_DEPLOY_XRAY_IPV4_IPV6_DNS_HTTPS_NATIVE_PASS
systemctl stop usque-cn
if ip route get 223.5.5.5 mark 5271 >/dev/null 2>&1; then exit 1; fi
if web https://114.114.114.114/ > "$work/blocked.txt" 2>/dev/null; then exit 1; fi
curl --noproxy '*' -fsS --max-time 15 --interface tun0 https://1.1.1.1/cdn-cgi/trace > "$work/old.txt"
grep -Eq '^warp=(on|plus)$' "$work/old.txt"
web https://1.1.1.1/cdn-cgi/trace > "$work/native-after-stop.txt"
grep -q '^warp=off$' "$work/native-after-stop.txt"
systemctl start usque-cn
old_pid=$(systemctl show usque-cn -p MainPID --value)
test "$old_pid" -gt 1
systemctl kill --kill-whom=main --signal=SIGKILL usque-cn
for _ in $(seq 1 40); do
  new_pid=$(systemctl show usque-cn -p MainPID --value)
  if test "$new_pid" -gt 1 && test "$new_pid" != "$old_pid" && systemctl is-active --quiet usque-cn; then break; fi
  sleep 1
done
test "$new_pid" -gt 1
test "$new_pid" != "$old_pid"
nodectl doctor --json > "$work/doctor-after-kill.json"
intact
echo REAL_CN_SYSTEMD_STOP_KILL_RESTART_INDEPENDENT_OLD_WARP_NATIVE_PASS
kill "$client"; wait "$client" || true
trap - EXIT
nodectl uninstall --execute > "$work/uninstall.txt" 2>&1
intact
curl --noproxy '*' -fsS http://127.0.0.1:9081/ | grep -q legacy-service
test -f /var/lib/nodectl-openlist/data.db
test ! -e /run/nodectl-cn-policy/owner.json
echo REAL_CN_SYSTEMD_UNINSTALL_RESTORES_ORIGINAL_HOST_RETAINS_DATA_PASS
echo "Private fixture logs: $work"
