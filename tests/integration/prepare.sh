#!/bin/bash
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
umask 077
test -x /usr/sbin/nginx
test -x /usr/local/bin/xray
install -m 0755 /integration-target/debug/nodectl /usr/local/bin/nodectl
id xray >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin xray
getent group www-data >/dev/null
install -d -m 0750 -o xray -g www-data /var/log/xray
install -d -m 0755 /var/www /var/www/html /var/www/acme /var/log/nginx
install -d -m 0700 /etc/ssl/dmit/domain /etc/ssl/dmit/ip /root/nodectl-test
printf 'integration-ready\n' > /var/www/html/index.html
chmod 0644 /var/www/html/index.html
openssl req -new -x509 -nodes -newkey ec -pkeyopt ec_paramgen_curve:P-384 -keyout /root/nodectl-test/ca.key -out /usr/local/share/ca-certificates/nodectl-test.crt -days 3 -subj /CN=nodectl-isolated-test-CA -addext basicConstraints=critical,CA:TRUE -addext keyUsage=critical,keyCertSign,cRLSign >/dev/null 2>&1
update-ca-certificates >/dev/null 2>&1
ip4=$(ip -4 -j address show dev eth0 | python3 -c 'import json,sys;print(json.load(sys.stdin)[0]["addr_info"][0]["local"])')
for kind in domain ip; do
  curve=P-384; [ "$kind" = ip ] && curve=prime256v1
  openssl req -new -nodes -newkey ec -pkeyopt "ec_paramgen_curve:$curve" -keyout "/etc/ssl/dmit/$kind/privkey.pem" -out "/root/nodectl-test/$kind.csr" -subj /CN=node.integration.test >/dev/null 2>&1
  if [ "$kind" = domain ]; then printf 'subjectAltName=DNS:node.integration.test,DNS:www.integration.test,DNS:reality.integration.test,DNS:probe.integration.test,IP:127.0.0.1\n' > /root/nodectl-test/extensions;
  else printf 'subjectAltName=IP:%s,IP:127.0.0.1,IP:::1\n' "$ip4" > /root/nodectl-test/extensions; fi
  printf 'basicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n' >> /root/nodectl-test/extensions
  openssl x509 -req -in "/root/nodectl-test/$kind.csr" -CA /usr/local/share/ca-certificates/nodectl-test.crt -CAkey /root/nodectl-test/ca.key -CAcreateserial -out "/etc/ssl/dmit/$kind/fullchain.pem" -days 3 -extfile /root/nodectl-test/extensions >/dev/null 2>&1
done
printf '127.0.0.1 node.integration.test www.integration.test reality.integration.test probe.integration.test\n' >> /etc/hosts
nodectl setup --non-interactive --domain node.integration.test --aliases www.integration.test --reality-domain reality.integration.test --ipv4 "$ip4" --ipv6 ::1 --execute >/dev/null
cat > /etc/nginx/nginx.conf <<'NGINX'
user www-data;
pid /run/nginx.pid;
events { worker_connections 256; }
http { server { listen 9081; location / { return 200 'legacy-service'; } } }
NGINX
systemctl enable nginx >/dev/null 2>&1
systemctl restart nginx
cp /etc/nginx/nginx.conf /root/nodectl-test/original-nginx.conf
echo PREPARE_PASS
