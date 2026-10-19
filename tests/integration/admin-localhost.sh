#!/bin/bash
set -euo pipefail
umask 077
test "${NODECTL_ADMIN_TEST:-}" = isolated
# .localhost is reserved for the local browser fixture. Some host proxy stacks
# intercept .test TLS/SNI despite an explicit Chromium DNS mapping.
sed -i 's/^domain = "node.integration.test"/domain = "node.localhost"/; s/^aliases = .*/aliases = ["node.integration.test", "www.integration.test"]/' /etc/dmit-node/node.toml
printf '127.0.0.1 node.localhost\n' >> /etc/hosts
openssl req -new -key /etc/ssl/dmit/domain/privkey.pem -out /root/nodectl-test/admin-localhost.csr -subj /CN=node.localhost >/dev/null 2>&1
printf 'subjectAltName=DNS:node.localhost,DNS:node.integration.test,DNS:www.integration.test,DNS:reality.integration.test,DNS:probe.integration.test,DNS:speedtest.integration.test,DNS:files.integration.test,IP:127.0.0.1\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n' > /root/nodectl-test/admin-localhost.extensions
openssl x509 -req -in /root/nodectl-test/admin-localhost.csr -CA /usr/local/share/ca-certificates/nodectl-test.crt -CAkey /root/nodectl-test/ca.key -CAcreateserial -out /etc/ssl/dmit/domain/fullchain.pem -days 3 -extfile /root/nodectl-test/admin-localhost.extensions >/dev/null 2>&1
