#!/bin/bash
set -euo pipefail
umask 077
test "${NODECTL_ADMIN_TEST:-}" = isolated
test -f /root/nodectl-test/admin-token
if NODECTL_FAILPOINT=uninstall-journal nodectl uninstall --execute > /root/nodectl-test/admin-uninstall-interruption.log 2>&1; then
  echo 'Expected the explicit isolated uninstall failpoint' >&2; exit 1
fi
grep -q 'Injected integration failure: uninstall-journal' /root/nodectl-test/admin-uninstall-interruption.log
test -f /var/lib/dmit-node/uninstall.json
