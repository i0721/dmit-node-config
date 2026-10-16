[Unit]
Description=Cloudflare WARP MASQUE via usque managed by nodectl
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory={{ WARP_DIR }}
ExecStart={{ USQUE_BINARY }} -c {{ WARP_CONFIG }} nativetun
Restart=always
RestartSec=5
DeviceAllow=/dev/net/tun rw
CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_RAW
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW

[Install]
WantedBy=multi-user.target
