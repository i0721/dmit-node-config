[Unit]
Description=Protected node distribution
After=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/nodectl share serve --from-systemd
Restart=on-failure
RestartSec=2
RuntimeDirectory=nodectl-share
RuntimeDirectoryMode=0750
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
PrivateDevices=true
PrivateNetwork=true
ProtectSystem=strict
ProtectHome=read-only
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_UNIX
CapabilityBoundingSet=CAP_CHOWN
TasksMax=16
MemoryMax=128M
LimitNOFILE=128
StandardOutput=null

[Install]
WantedBy=multi-user.target
