[Unit]
Description=LibreSpeed measurement backend
After=network-online.target

[Service]
Type=exec
User=nodectl-speed
Group=nodectl-speed
LoadCredential=settings.toml:/etc/nodectl-services/librespeed.toml
ExecStart=/usr/local/bin/speedtest-backend -c %d/settings.toml
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
PrivateDevices=true
ProtectSystem=strict
ProtectHome=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
CapabilityBoundingSet=
StandardOutput=null

[Install]
WantedBy=multi-user.target
