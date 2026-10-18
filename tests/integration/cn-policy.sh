#!/bin/bash
# Kernel policy test in fresh mount + network namespaces. No production routes.
set -euo pipefail
if [ "${1:-}" != --inside ]; then
  binary=$(realpath "${1:?path to freshly built nodectl}")
  exec unshare --mount --net --fork bash "$0" --inside "$binary"
fi
binary=${2:?binary}
mount --make-rprivate /
mount -t tmpfs tmpfs /run
ip link set lo up
ip link add native0 type dummy
ip addr add 192.0.2.2/24 dev native0
ip link set native0 up
ip route add default dev native0
args=(--interface warp-cn --table 5271 --priority 15271 --mark 5271)
refused() {
  if "$@" >/dev/null 2>&1; then
    echo 'Marked traffic unexpectedly escaped rejection' >&2
    exit 1
  fi
}
"$binary" cn-route prepare "${args[@]}"
refused ip route get 8.8.8.8 mark 5271
ip route get 8.8.8.8 | grep -q 'dev native0'
ip link add warp-cn type dummy
ip addr add 172.16.0.2/32 dev warp-cn
ip -6 addr add 2606:4700:110:8000::2/128 dev warp-cn nodad
ip link set warp-cn up
"$binary" cn-route ready "${args[@]}"
ip route get 8.8.8.8 mark 5271 | grep -q 'dev warp-cn'
ip -6 route get 2606:4700:4700::1111 mark 5271 | grep -q 'dev warp-cn'
ip route get 8.8.8.8 | grep -q 'dev native0'
"$binary" cn-route block "${args[@]}"
refused ip route get 8.8.8.8 mark 5271
ip link set warp-cn up
"$binary" cn-route ready "${args[@]}"
ip link del warp-cn
refused ip route get 8.8.8.8 mark 5271
refused ip -6 route get 2606:4700:4700::1111 mark 5271
ip route get 8.8.8.8 | grep -q 'dev native0'
ip -j route show default | grep -q native0
# Foreign drift must survive the refused reconciliation unchanged.
ip route add 198.51.100.0/24 dev native0 table 5271
refused "$binary" cn-route prepare "${args[@]}"
ip route show table 5271 | grep -q 198.51.100.0/24
echo CN_POLICY_IPV4_IPV6_MARK_BLOCK_INTERFACE_DELETE_NATIVE_DEFAULT_FOREIGN_DRIFT_PASS
