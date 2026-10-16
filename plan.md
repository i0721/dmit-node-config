> **裸机 Linux + systemd + Nginx + 原始 Xray Core + acme.sh + usque，完全抛弃 Docker / 3x-ui。**
>
> 443：XHTTP（你说的 SplitHTTP）80% + WS 15%  
> 8443：XHTTP + REALITY 5%  
> Nginx：TLS 1.3 Only + H2 + H3 + `Alt-Svc always`  
> Nginx ↔ Xray：两个 UDS  
> REALITY target：`127.0.0.1:443`  
> WARP：单独运行 usque，先 **TUN 不接管系统路由** 做调试。

另外我核对了当前版本：截至 **2026-10-16**，Xray 最新 pre-release 已经是 **v26.10.10**；当前官方传输配置明确支持 `XHTTP + TLS`、`XHTTP + REALITY`，所以这里不需要使用 Trojan / AnyTLS 等额外协议。[GitHub](https://github.com/XTLS/Xray-core/releases?after=v1.3.1\&utm_source=chatgpt.com)

---

# 1. 最终结构

```text
                         Internet
                            │
              ┌─────────────┴─────────────┐
              │                           │
          TCP :443                    TCP :8443
          UDP :443                       │
              │                          │
            Nginx                      Xray
              │                          │
       ┌──────┴──────┐             REALITY
       │             │                 │
   资源子路径      事件子路径             │
       │             │                 │
       │             │          target 127.0.0.1:443
       │             │                 │
       ▼             ▼                 ▼
 /tmp/vless-    /tmp/vless-          Nginx
 xhttp.socket   ws.socket              :443
       │             │
       └──────┬──────┘
              │
             Xray
              │
          Freedom 出站
```

### 流量比例

```text
100%
│
├── 80%  VLESS + XHTTP + TLS :443
│       └── fingerprint
│           Chrome : iOS : Firefox : QQ
│           5 : 2 : 3 : 1
│
├── 15%  VLESS + WS + TLS :443
│
└── 5%   VLESS + XHTTP + REALITY :8443
```

这里的 **80/15/5 是客户端 Profile 分配问题，不是 Xray 服务端去“随机分流”**。

---

# 2. 一个非常重要的修正

你一直说：

```text
SplitHTTP
```

现在 Xray Core 的配置名称已经是：

```json
"network": "xhttp"
```

也就是现在应该按照 **XHTTP** 配。

官方 Xray 示例目前也直接使用：

```json
"network": "xhttp"
```

并提供 `VLESS-XHTTP3-Nginx` 官方示例。[GitHub](https://github.com/XTLS/Xray-examples/blob/main/VLESS-XHTTP3-Nginx/server.jsonc?utm_source=chatgpt.com)

---

# 3. DNS

假设：

```text
主域：
example.com

REALITY SNI：
reality.example.com
```

那么：

```text
@       A     DMIT IPv4
www     A     DMIT IPv4

@       AAAA  DMIT IPv6
www     AAAA  DMIT IPv6

reality A     DMIT IPv4
reality AAAA  DMIT IPv6
```

也就是说：

```text
example.com
www.example.com
reality.example.com
```

全部直接指向 **你自己的 DMIT IP**。

不要拿其他人的域名、证书或者 IP 段上的证书来拼。

---

# 4. 两套证书

## 证书 A：域名证书

至少：

```text
DNS:example.com
DNS:www.example.com
DNS:reality.example.com
```

使用：

```text
ECDSA P-384
```

acme.sh 对 `ec-384` 有原生支持。[GitHub](https://github.com/acmesh-official/acme.sh?utm_source=chatgpt.com)

例如：

```bash
acme.sh --issue \
  --server letsencrypt \
  --dns dns_cf \
  -d example.com \
  -d www.example.com \
  -d reality.example.com \
  --keylength ec-384
```

如果你的 DNS 不是 Cloudflare，把 `dns_cf` 换成对应 DNS API。

---

# 5. IPSSL 证书

第二张：

```text
IP Address: DMIT IPv4
IP Address: DMIT IPv6
```

例如：

```text
IP Address: 203.0.113.10
IP Address: 2001:db8::10
```

当前 acme.sh 已经加入 IP identifier 支持；Let's Encrypt 的 IP 证书属于短生命周期证书，需要按照 CA 当前 IP 证书 profile 的要求申请。acme.sh 当前代码也已经包含 `ID_TYPE_IP`，其项目 Wiki 示例使用 `shortlived` profile。[GitHub](https://github.com/acmesh-official/acme.sh/blob/master/acme.sh?utm_source=chatgpt.com)

所以这里不要把：

```text
IP证书
```

和普通：

```text
example.com
www.example.com
```

证书混成一张。

---

# 6. 最终证书布局

服务器：

```text
/etc/ssl/dmit/
├── domain/
│   ├── fullchain.pem
│   └── privkey.pem
│
└── ipssl/
    ├── fullchain.pem
    └── privkey.pem
```

最终：

```text
domain/fullchain.pem
├── example.com
├── www.example.com
└── reality.example.com


ipssl/fullchain.pem
├── DMIT IPv4
└── DMIT IPv6
```

---

# 7. REALITY SAN 逻辑

你要求的是：

> REALITY SANList = 两套证书 SAN 的并集。

逻辑上就是：

```text
example.com
www.example.com
reality.example.com
DMIT IPv4
DMIT IPv6
```

但是注意：

**IP 不是普通 TLS SNI。**

Xray 当前 TLS 文档明确说明：如果客户端 `serverName` 填 IP，TLS ClientHello 不会发送 SNI；域名才会作为 SNI 发送。[Project X](https://xtls.github.io/config/transports/tls.html?utm_source=chatgpt.com)

因此客户端：

```text
Profile A
serverName = example.com
```

发送：

```text
SNI: example.com
```

而：

```text
Profile B
serverName = 203.0.113.10
```

不会发送：

```text
SNI: 203.0.113.10
```

而是无 SNI。

所以 **不要把“IP SAN”误认为“IP SNI”**。

---

# 8. 安装基础环境

以 Debian/Ubuntu 为例：

```bash
apt update

apt install -y \
    curl \
    wget \
    git \
    ca-certificates \
    openssl \
    build-essential \
    libpcre2-dev \
    zlib1g-dev \
    libssl-dev \
    iproute2 \
    jq
```

创建用户：

```bash
useradd \
    --system \
    --no-create-home \
    --shell /usr/sbin/nologin \
    xray
```

让 Xray 可以创建 Nginx 能访问的 UDS：

```bash
usermod -aG www-data xray
```

---

# 9. 编译当前 Xray

当前最新 pre-release 是：

```text
v26.10.10
```

发布日期为 2026-10-10。[GitHub](https://github.com/XTLS/Xray-core/releases?after=v1.3.1\&utm_source=chatgpt.com)

建议固定这个版本，而不是无脑跟随未来 `main`：

```bash
cd /usr/local/src

git clone \
    --depth 1 \
    --branch v26.10.10 \
    https://github.com/XTLS/Xray-core.git

cd Xray-core
```

检查 Go：

```bash
go version
```

当前 v26.10.10 的实际构建记录显示使用 Go 1.27.2。[GitHub](https://github.com/XTLS/Xray-core/issues/7164?utm_source=chatgpt.com)

然后：

```bash
CGO_ENABLED=0 \
go build \
    -trimpath \
    -buildvcs=false \
    -ldflags="-s -w" \
    -o /usr/local/bin/xray \
    ./main
```

检查：

```bash
/usr/local/bin/xray version
```

---

# 10. Xray 配置目录

```bash
mkdir -p /etc/xray

chown -R root:xray /etc/xray
chmod 750 /etc/xray
```

---

# 11. 生成 UUID

```bash
/usr/local/bin/xray uuid
```

生成三个 UUID：

```text
XHTTP_UUID
WS_UUID
REALITY_UUID
```

不要三个入站共用一个 UUID。

---

# 12. 生成 REALITY Key

```bash
/usr/local/bin/xray x25519
```

得到：

```text
Private key:
Public key:
```

保存：

```text
Private Key → 服务端
Public Key  → 客户端
```

---

# 13. 生成 Short ID

```bash
openssl rand -hex 8
```

例如：

```text
8f3a9c20b41d6e82
```

---

# 14. Xray 最终配置

### 入口路径约定

两个入口放在多级业务路径下，不使用直接暴露协议名称的根级路径：

| 用途 | 本文示例路径 |
| --- | --- |
| XHTTP（443） | `/assets/modules/6b91d4e8a37fc205/stream/` |
| WS（443） | `/api/events/c84f20a7d693be51/channel` |

部署时分别运行一次 `openssl rand -hex 8`，用生成的值替换上表中的两个随机段，并同步替换下文 **Xray、Nginx 和客户端 Profile** 的对应路径。XHTTP 路径末尾的 `/` 也要保持一致。这些是 URL 路由，不需要在网站目录下创建对应的文件或文件夹；普通网站内容仍由原来的 `location /` 处理。路径命名只用于减少明显的协议标识，不替代认证。

`/etc/xray/config.json`

```json
{
  "log": {
    "loglevel": "warning",
    "access": "/var/log/xray/access.log",
    "error": "/var/log/xray/error.log"
  },

  "inbounds": [

    {
      "tag": "vless-xhttp-443",

      "listen": "/tmp/vless-xhttp.socket,0660",

      "protocol": "vless",

      "settings": {
        "clients": [
          {
            "id": "XHTTP_UUID"
          }
        ],
        "decryption": "none"
      },

      "streamSettings": {
        "network": "xhttp",

        "xhttpSettings": {
          "path": "/assets/modules/6b91d4e8a37fc205/stream/",
          "mode": "stream-one"
        }
      }
    },


    {
      "tag": "vless-ws-443",

      "listen": "/tmp/vless-ws.socket,0660",

      "protocol": "vless",

      "settings": {
        "clients": [
          {
            "id": "WS_UUID"
          }
        ],
        "decryption": "none"
      },

      "streamSettings": {
        "network": "ws",

        "wsSettings": {
          "path": "/api/events/c84f20a7d693be51/channel"
        }
      }
    },


    {
      "tag": "vless-reality-xhttp-8443",

      "listen": "0.0.0.0",
      "port": 8443,

      "protocol": "vless",

      "settings": {
        "clients": [
          {
            "id": "REALITY_UUID"
          }
        ],
        "decryption": "none"
      },

      "streamSettings": {
        "network": "xhttp",

        "xhttpSettings": {
          "path": "/reality",
          "mode": "stream-one"
        },

        "security": "reality",

        "realitySettings": {
          "show": false,

          "target": "127.0.0.1:443",

          "xver": 0,

          "serverNames": [
            "example.com",
            "www.example.com",
            "reality.example.com",
            "203.0.113.10",
            "2001:db8::10"
          ],

          "privateKey": "REALITY_PRIVATE_KEY",

          "shortIds": [
            "REALITY_SHORT_ID"
          ]
        }
      }
    }

  ],


  "outbounds": [
    {
      "tag": "direct",
      "protocol": "freedom"
    },

    {
      "tag": "block",
      "protocol": "blackhole"
    }
  ]
}
```

当前 Core 的配置表明确支持：

```text
XHTTP + TLS
XHTTP + REALITY
```

而 WS 不支持 REALITY，因此你的 WS 放在 Nginx TLS 后面是正确结构。[Project X](https://xtls.github.io/config/transport.html?utm_source=chatgpt.com)

---

# 15. 注意 REALITY + 本地 Nginx 这一点

这里必须给你一个非常重要的警告。

你的要求：

```text
REALITY
  ↓
target = 127.0.0.1:443
  ↓
Nginx
```

**当前 Xray 社区存在 XHTTP + REALITY + 本地 Nginx target 的已知问题记录。**

2026 年的 issue #5923 中，作者报告：

```text
XHTTP + REALITY
target = local Nginx
```

出现 `unexpected ccs message`；

而：

```text
TCP + REALITY
target = local Nginx
```

正常。

这个 issue 最终被 Core 开发者关闭为 `not planned`。[GitHub](https://github.com/XTLS/Xray-core/issues/5923)

所以：

> **你的要求可以作为实验配置，但我不能把 `XHTTP + REALITY → 127.0.0.1:443 Nginx` 描述成“官方已经保证稳定”的配置。**

这也是为什么这里我建议你使用当前 pre-release `v26.10.10`，先做真实客户端测试，而不是拿某个旧教程的 Xray 二进制直接上线。

---

# 16. 编译 Nginx

你明确要求：

```text
TLS 1.3
H2
H3
```

所以不要依赖一个不知道是否编译了 HTTP/3 的旧发行版 Nginx。

Nginx 官方 HTTP/3 模块需要：

```text
--with-http_v3_module
```

并且官方当前配置方式是：

```nginx
listen 443 quic reuseport;
listen 443 ssl;
http2 on;
http3 on;
``` :chatgpt-content-reference{index="9"}


例如：

```bash
cd /usr/local/src

wget https://nginx.org/download/nginx-1.31.7.tar.gz

tar xf nginx-1.31.7.tar.gz

cd nginx-1.31.7
```

编译：

```bash
./configure \
    --prefix=/usr/local/nginx \
    --sbin-path=/usr/sbin/nginx \
    --conf-path=/etc/nginx/nginx.conf \
    --pid-path=/run/nginx.pid \
    --lock-path=/run/lock/nginx.lock \
    --error-log-path=/var/log/nginx/error.log \
    --http-log-path=/var/log/nginx/access.log \
    --with-http_ssl_module \
    --with-http_v2_module \
    --with-http_v3_module \
    --with-http_realip_module \
    --with-threads

make -j"$(nproc)"

make install
```

复制 MIME：

```bash
mkdir -p /etc/nginx

cp conf/mime.types /etc/nginx/mime.types
```

检查：

```bash
nginx -V 2>&1 | grep -o -- '--with-http_[a-z0-9_]*_module'
```

必须看到：

```text
--with-http_ssl_module
--with-http_v2_module
--with-http_v3_module
```

---

# 17. Nginx 目录

```bash
mkdir -p \
    /etc/nginx \
    /var/www/html \
    /var/www/acme \
    /etc/ssl/dmit/domain \
    /etc/ssl/dmit/ipssl
```

网站：

```bash
cat >/var/www/html/index.html <<'EOF'
<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>Welcome</title>
</head>
<body>
<h1>Welcome</h1>
</body>
</html>
EOF
```

---

# 18. Nginx 核心配置

`/etc/nginx/nginx.conf`

```nginx
user www-data;

worker_processes auto;

error_log /var/log/nginx/error.log warn;

pid /run/nginx.pid;


events {
    worker_connections 4096;
}


http {

    include /etc/nginx/mime.types;

    default_type application/octet-stream;

    sendfile on;

    keepalive_timeout 65;


    # ==================================================
    # TLS
    # ==================================================

    ssl_protocols TLSv1.3;

    ssl_session_cache shared:SSL:20m;

    ssl_session_timeout 10m;

    ssl_session_tickets off;


    # ==================================================
    # HTTP/2
    # ==================================================

    http2 on;


    # ==================================================
    # HTTP/3
    # ==================================================

    http3 on;


    # ==================================================
    # 80
    # ACME + HTTP redirect
    # ==================================================

    server {

        listen 80 default_server;
        listen [::]:80 default_server;

        server_name _;


        location ^~ /.well-known/acme-challenge/ {
            root /var/www/acme;
        }


        location / {
            return 301 https://$host$request_uri;
        }
    }


    # ==================================================
    # 443 DEFAULT
    #
    # 非法 SNI / IP 直接访问
    # 使用 IPSSL
    # ==================================================

    server {

        listen 443 ssl default_server;

        listen [::]:443 ssl default_server;


        # QUIC / HTTP3
        listen 443 quic reuseport default_server;

        listen [::]:443 quic reuseport default_server;


        server_name _;


        ssl_certificate
            /etc/ssl/dmit/ipssl/fullchain.pem;

        ssl_certificate_key
            /etc/ssl/dmit/ipssl/privkey.pem;


        add_header Alt-Svc
            'h3=":443"; ma=86400'
            always;


        location / {

            root /var/www/html;

            index index.html;

            try_files
                $uri
                $uri/
                /index.html;
        }
    }


    # ==================================================
    # 443 MAIN DOMAIN
    #
    # example.com
    # www.example.com
    # reality.example.com
    # ==================================================

    server {

        listen 443 ssl;

        listen [::]:443 ssl;


        listen 443 quic;

        listen [::]:443 quic;


        server_name
            example.com
            www.example.com
            reality.example.com;


        ssl_certificate
            /etc/ssl/dmit/domain/fullchain.pem;

        ssl_certificate_key
            /etc/ssl/dmit/domain/privkey.pem;


        # TLS 1.3 Only
        ssl_protocols TLSv1.3;


        # ==================================================
        # HTTP/3
        # ==================================================

        add_header Alt-Svc
            'h3=":443"; ma=86400'
            always;


        # ==================================================
        # XHTTP
        # ==================================================

        # 保留 XHTTP 子路径，原样转发完整 URI。
        location ^~ /assets/modules/6b91d4e8a37fc205/stream/ {

            client_max_body_size 0;

            client_body_timeout 1h;

            grpc_read_timeout 1h;

            grpc_send_timeout 1h;

            grpc_set_header
                X-Forwarded-For
                $proxy_add_x_forwarded_for;

            grpc_set_header
                X-Real-IP
                $remote_addr;

            grpc_pass
                unix:/tmp/vless-xhttp.socket;
        }


        # ==================================================
        # WebSocket
        # ==================================================

        location = /api/events/c84f20a7d693be51/channel {

            proxy_http_version 1.1;

            proxy_set_header
                Upgrade
                $http_upgrade;

            proxy_set_header
                Connection
                "upgrade";

            proxy_set_header
                Host
                $host;

            proxy_set_header
                X-Real-IP
                $remote_addr;

            proxy_set_header
                X-Forwarded-For
                $proxy_add_x_forwarded_for;

            proxy_set_header
                X-Forwarded-Proto
                $scheme;


            proxy_read_timeout 1h;

            proxy_send_timeout 1h;

            proxy_buffering off;

            proxy_request_buffering off;


            proxy_pass
                http://unix:/tmp/vless-ws.socket:;
        }


        # ==================================================
        # 正常网站
        # ==================================================

        location / {

            root /var/www/html;

            index index.html;

            try_files
                $uri
                $uri/
                /index.html;
        }
    }
}
```

这里 XHTTP 使用 `grpc_pass` 是有现成 Xray 官方示例和社区配置依据的；Xray 官方示例目录本身就是 `VLESS-XHTTP3-Nginx`。[GitHub](https://github.com/XTLS/Xray-examples/blob/main/VLESS-XHTTP3-Nginx/server.jsonc?utm_source=chatgpt.com)

---

# 19. 关于 `reuseport`

注意我没有在每一个：

```nginx
listen 443 quic;
```

后面重复写：

```text
reuseport
```

因为 Nginx 对同一个 `address:port` 的 socket-level listen 选项要求谨慎处理，重复指定可能产生 `duplicate listen options`。[Nginx](https://trac.nginx.org/nginx/ticket/1912?utm_source=chatgpt.com)

所以：

```nginx
listen 443 quic reuseport default_server;
```

只负责建立这个 UDP socket。

其他 server：

```nginx
listen 443 quic;
```

即可。

---

# 20. 为什么非法域名 / IP 会拿到 IPSSL

这里：

```nginx
server {
    listen 443 ssl default_server;
    ...
    ssl_certificate /etc/ssl/dmit/ipssl/fullchain.pem;
    ...
}
```

意味着：

```text
https://203.0.113.10
        ↓
无匹配 SNI
        ↓
default_server
        ↓
IPSSL
```

以及：

```text
https://random.example.net
        ↓
SNI = random.example.net
        ↓
没有匹配 server_name
        ↓
default_server
        ↓
IPSSL
```

而：

```text
example.com
www.example.com
reality.example.com
```

则进入域名证书对应的虚拟主机。

这正好符合你的要求。

---

# 21. Xray systemd

创建：

```bash
cat >/etc/systemd/system/xray.service <<'EOF'
[Unit]
Description=Xray Core
After=network-online.target
Wants=network-online.target

[Service]
Type=simple

User=xray
Group=xray
SupplementaryGroups=www-data

ExecStartPre=/usr/local/bin/xray run -test -config /etc/xray/config.json
ExecStart=/usr/local/bin/xray run -config /etc/xray/config.json

Restart=on-failure
RestartSec=3

UMask=0007

LimitNOFILE=1048576

NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF
```

---

# 22. Xray 日志

```bash
mkdir -p /var/log/xray

chown -R xray:xray /var/log/xray
```

---

# 23. Nginx systemd

```bash
cat >/etc/systemd/system/nginx.service <<'EOF'
[Unit]
Description=NGINX HTTP Server
After=network.target
Wants=network-online.target

[Service]
Type=forking

PIDFile=/run/nginx.pid

ExecStartPre=/usr/sbin/nginx -t
ExecStart=/usr/sbin/nginx

ExecReload=/usr/sbin/nginx -s reload

ExecStop=/usr/sbin/nginx -s quit

PrivateTmp=false

Restart=on-failure

[Install]
WantedBy=multi-user.target
EOF
```

---

# 24. 启动顺序

先：

```bash
systemctl daemon-reload
```

测试 Xray：

```bash
xray run -test -config /etc/xray/config.json
```

测试 Nginx：

```bash
nginx -t
```

然后：

```bash
systemctl enable --now xray
systemctl enable --now nginx
```

---

# 25. 检查两个 Socket

```bash
ls -lah \
    /tmp/vless-xhttp.socket \
    /tmp/vless-ws.socket
```

应该类似：

```text
srw-rw---- xray www-data /tmp/vless-xhttp.socket
srw-rw---- xray www-data /tmp/vless-ws.socket
```

如果没有：

```bash
journalctl -u xray -n 100 --no-pager
```

---

# 26. 检查监听端口

```bash
ss -lntup
```

应该看到：

```text
*:443
*:8443
```

再：

```bash
ss -lunp
```

应该看到：

```text
*:443
```

这里的 UDP 443 是：

```text
Nginx HTTP/3 / QUIC
```

**不是你所说的“部署 Xray UDP 协议”。**

Nginx 官方 HTTP/3 本身就是 QUIC/UDP，因此你要求 H3 就必然需要 UDP 443。[Nginx](https://nginx.org/en/docs/http/ngx_http_v3_module.html?utm_source=chatgpt.com)

---

# 27. 客户端 Profile

## Profile 1：80% XHTTP

```text
Address:
example.com

Port:
443

Protocol:
VLESS

UUID:
XHTTP_UUID

Encryption:
none

Network:
XHTTP

Path:
/assets/modules/6b91d4e8a37fc205/stream/

Security:
TLS

SNI:
example.com
```

ALPN：

```text
h3
h2
http/1.1
```

Fingerprint：

```text
chrome
```

---

# 28. XHTTP 指纹比例

你要求：

```text
Chrome : iOS : Firefox : QQ
5 : 2 : 3 : 1
```

所以客户端配置生成器分配：

```text
11 个 Profile

5 × chrome
2 × ios
3 × firefox
1 × qq
```

也就是：

```text
45.45% chrome
18.18% ios
27.27% firefox
9.09% qq
```

这是 **XHTTP 的 80% 人群内部**的比例。

Xray 官方支持这些 uTLS fingerprint：

```text
chrome
firefox
ios
qq
```

但一定注意：

> fingerprint 只模拟 TLS ClientHello；它**不是完整浏览器行为模拟**。

Xray 官方对此明确提醒，其他行为仍然可能保持 Go/Xray 自身特征，所以不要把 `fingerprint=chrome` 理解成“整个连接就等于 Chrome”。[Project X](https://xtls.github.io/config/transports/tls.html?utm_source=chatgpt.com)

---

# 29. Profile 2：15% WS

```text
Address:
example.com

Port:
443

Protocol:
VLESS

UUID:
WS_UUID

Network:
WS

Path:
/api/events/c84f20a7d693be51/channel

Security:
TLS

SNI:
example.com

ALPN:
http/1.1
```

这里不要给 WS 强制：

```text
h2
```

因为 WebSocket 正常情况下使用 HTTP/1.1；Xray 官方 TLS 文档也特别说明 WS 默认使用 `http/1.1`，否则协商到 h2 会导致 WS 无法建立。[Project X](https://xtls.github.io/config/transports/tls.html?utm_source=chatgpt.com)

---

# 30. Profile 3：5% REALITY

```text
Address:
你的服务器地址

Port:
8443

Protocol:
VLESS

UUID:
REALITY_UUID

Network:
XHTTP

Security:
REALITY

SNI:
reality.example.com

PublicKey:
REALITY_PUBLIC_KEY

ShortID:
REALITY_SHORT_ID

Fingerprint:
chrome
```

关键：

```text
443:
example.com / www.example.com

8443:
reality.example.com
```

不要让两个入口使用相同的主 SNI。

而：

```text
example.com
www.example.com
```

仍然视为同一个主域的两个 DNS 名称。

---

# 31. 这里不要做“服务端 SNI 随机”

你的这个理解是对的：

```text
Server
    ↓
只负责接受合法 serverNames

Client Profile
    ↓
决定具体使用：
example.com
www.example.com
reality.example.com
或者 IP Profile
```

服务端不要人为：

```text
今天 example.com
明天 www.example.com
随机 SNI
```

客户端 Profile 才负责具体选择。

---

# 32. usque：单独接入 WARP

当前 usque 官方项目是：

> Cloudflare WARP MASQUE 的开源重新实现。

支持：

```text
nativetun
socks
http-proxy
l4-socks
l4-http-proxy
portfw
```

Linux 的 `nativetun` 会创建真正的 TUN 网卡，但官方文档明确说明：

> **它不会自动设置路由。**

这恰好符合你说的：

> `usquetun+不注册路由方便 debug` [GitHub](https://github.com/Diniboy1123/usque/blob/main/README.md?utm_source=chatgpt.com)


---

# 33. 安装 usque

当前官方仓库：

```text
Diniboy1123/usque
```

Linux amd64 有 release binary；源码构建至少需要 Go 1.26.3。[GitHub](https://github.com/Diniboy1123/usque/blob/main/README.md?utm_source=chatgpt.com)

如果现场编译：

```bash
cd /usr/local/src

git clone \
    https://github.com/Diniboy1123/usque.git

cd usque

CGO_ENABLED=0 \
go build \
    -ldflags="-s -w" \
    -o /usr/local/bin/usque \
    .
```

---

# 34. 注册 WARP

创建：

```bash
mkdir -p /etc/usque
```

然后：

```bash
cd /etc/usque

/usr/local/bin/usque register
```

官方 `register` 会创建新的 WARP 配置，并完成设备注册和 MASQUE enrollment。[GitHub](https://github.com/Diniboy1123/usque?utm_source=chatgpt.com)

---

# 35. usque systemd

先**不添加任何路由 Hook**。

```bash
cat >/etc/systemd/system/usque-warp.service <<'EOF'
[Unit]
Description=Cloudflare WARP MASQUE via usque
After=network-online.target
Wants=network-online.target

[Service]
Type=simple

WorkingDirectory=/etc/usque

ExecStart=/usr/local/bin/usque \
    -c /etc/usque/config.json \
    nativetun

Restart=always
RestartSec=5

DeviceAllow=/dev/net/tun rw

CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_RAW
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW

[Install]
WantedBy=multi-user.target
EOF
```

然后：

```bash
systemctl daemon-reload

systemctl enable --now usque-warp
```

---

# 36. 检查 WARP TUN

```bash
ip addr
```

应该看到类似：

```text
tun0
```

或者：

```text
tun1
```

检查：

```bash
ip route
```

你应该发现：

> **默认路由仍然是 DMIT 原来的路由。**

这正是你现在想要的 debug 模式。

---

# 37. 单独测试 WARP

例如：

```bash
curl --interface tun0 \
    https://cloudflare.com/cdn-cgi/trace
```

如果你的接口叫：

```text
tun1
```

就：

```bash
curl --interface tun1 \
    https://cloudflare.com/cdn-cgi/trace
```

重点看：

```text
warp=
colo=
ip=
```

再测试 IPv6：

```bash
curl -6 \
    --interface tun0 \
    https://cloudflare.com/cdn-cgi/trace
```

---

# 38. 为什么现在不要让 usque 接管默认路由

你的要求是：

```text
usque
 ↓
TUN
 ↓
不注册路由
 ↓
debug
```

这非常合理。

因为一上来：

```text
WARP
 ↓
default route
 ↓
整台 DMIT
 ↓
所有 Nginx/Xray/SSH/DNS
```

一旦 WARP 出问题，很容易直接：

```text
SSH 断
DNS 断
apt 断
Xray 断
acme 断
```

所以第一阶段：

```text
DMIT 原始网络
    │
    ├── Nginx
    ├── Xray
    └── acme.sh
       
WARP
    │
    └── tun0
          │
          └── 仅 curl / debug
```

更稳。

---

# 39. 后续如果要让 Xray 出口经过 WARP

不要直接：

```text
WARP → 替换整个 DMIT default route
```

而是建议：

```text
Xray
  │
  ├── 普通 direct
  │
  └── WARP
       │
       └── 独立策略路由
```

或者：

```text
Xray
 ↓
127.0.0.1:1080
 ↓
usque socks
 ↓
WARP MASQUE
```

这两种都比“整个 VPS 默认路由切 WARP”更容易维护。

而且当前 Xray 官方出站协议列表里并没有一个可以简单写成：

```json
"protocol": "masque"
```

的独立出站协议；不要照网上旧教程随便塞一个 `masque` outbound。当前文档列出的出站协议仍是 Freedom、SOCKS、VLESS、WireGuard、Hysteria 等。[Project X](https://xtls.github.io/config/outbound.html?utm_source=chatgpt.com)

---

# 40. 最终 systemd 服务

最终就是三个核心服务：

```text
systemctl status nginx
systemctl status xray
systemctl status usque-warp
```

关系：

```text
                  DMIT VPS
                     │
       ┌─────────────┼─────────────┐
       │             │             │
     Nginx          Xray         usque
       │             │             │
     :443          :8443         tun0
       │             │             │
 ┌─────┴─────┐      │       不接管默认路由
 │           │      │
XHTTP       WS    REALITY
 │           │      │
UDS         UDS     │
 │           │      │
 └─────┬─────┘      │
       │             │
      Xray           │
       │             │
    Freedom          │
       │             │
       └─────────────┘
```

---

# 41. 最终检查表

上线之前逐项确认：

```text
[ ] DMIT IPv4 / IPv6 都是自己的
[ ] @ → DMIT
[ ] www → DMIT
[ ] reality → DMIT

[ ] 没有使用第三方证书
[ ] 没有盗用其他 IP 的证书
[ ] 域名证书包含 @ + www
[ ] 域名证书使用 EC P-384
[ ] IPSSL SAN 包含 IPv4 + IPv6

[ ] Xray 使用原始 Core
[ ] Xray v26.10.10 pre-release
[ ] Xray 现场 gobuild
[ ] 没有 3x-ui
[ ] 没有 Docker

[ ] /tmp/vless-xhttp.socket
[ ] /tmp/vless-ws.socket

[ ] 443 XHTTP
[ ] 443 WS
[ ] 8443 XHTTP + REALITY
[ ] REALITY target = 127.0.0.1:443

[ ] 443 主 SNI ≠ 8443 主 SNI
[ ] @ / www 视为同一主域
[ ] REALITY serverNames 按两套证书 SAN 逻辑并集
[ ] IP 不当成普通 TLS SNI

[ ] Nginx TLS 1.3 Only
[ ] Nginx H2
[ ] Nginx H3
[ ] Alt-Svc always
[ ] 非法 SNI → IPSSL default_server
[ ] IP 访问 → IPSSL default_server

[ ] usque MASQUE
[ ] nativetun
[ ] 暂不修改 default route
[ ] curl --interface tunX 验证 WARP
```
