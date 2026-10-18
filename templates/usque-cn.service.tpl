[Unit]
Description=Independent CN egress tunnel
After=network-online.target
Wants=network-online.target

[Service]
Type=exec
UMask=0077
RuntimeDirectory=nodectl-cn-policy
RuntimeDirectoryMode=0700
RuntimeDirectoryPreserve=yes
# Command capture needs writable private temporary files under strict mounts.
Environment=TMPDIR=/run/nodectl-cn-policy
ExecStartPre=/usr/local/bin/nodectl cn-route prepare --interface {{ CN_INTERFACE }} --table {{ CN_TABLE }} --priority {{ CN_PRIORITY }} --mark {{ CN_MARK }}
ExecStart={{ USQUE_BINARY }} -c {{ CN_CONFIG }} nativetun --interface-name {{ CN_INTERFACE }} --mtu 1280 --reconnect-delay 3s --keepalive-period 25s --connect-port 8443 --sni-address consumer-masque.cloudflareclient.com --always-reconnect
ExecStartPost=/usr/local/bin/nodectl cn-route ready --interface {{ CN_INTERFACE }} --table {{ CN_TABLE }} --priority {{ CN_PRIORITY }} --mark {{ CN_MARK }}
ExecStopPost=/usr/local/bin/nodectl cn-route block --interface {{ CN_INTERFACE }} --table {{ CN_TABLE }} --priority {{ CN_PRIORITY }} --mark {{ CN_MARK }}
Restart=always
RestartSec=5
DeviceAllow=/dev/net/tun rw
CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_RAW
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadOnlyPaths={{ CN_CONFIG }}
StandardOutput=null
StandardError=null

[Install]
WantedBy=multi-user.target
