#!/bin/bash
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
source /etc/os-release
curl -fsS https://nginx.org/keys/nginx_signing.key -o /tmp/nginx.key
gpg --show-keys --with-colons /tmp/nginx.key | grep -q 573BFD6B3D8FBC641079A6ABABF5BD827BD9BF62
gpg --batch --yes --dearmor -o /usr/share/keyrings/nginx-archive-keyring.gpg /tmp/nginx.key
chmod 0644 /usr/share/keyrings/nginx-archive-keyring.gpg
printf 'deb [signed-by=/usr/share/keyrings/nginx-archive-keyring.gpg] https://nginx.org/packages/%s %s nginx\n' "$ID" "$VERSION_CODENAME" > /etc/apt/sources.list.d/nginx.list
apt-get update
apt-get install -y nginx
/integration-target/debug/repo-tools download-xray /usr/local/bin/xray
/integration-target/debug/repo-tools download-mihomo /usr/local/bin/mihomo
