#!/bin/bash
set -euo pipefail
# systemd credentials use child mount namespaces; propagation must remain within
# this container's originally private mount tree, as required by its interface.
mount --make-rshared /
mount -o remount,ro /sys
mount --bind /proc/sys /proc/sys
mount -o remount,bind,ro /proc/sys
exec "$@"
