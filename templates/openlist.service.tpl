[Unit]
Description=OpenList file storage
After=network-online.target

[Service]
Type=exec
User=nodectl-files
Group=nodectl-files
WorkingDirectory={{ OPENLIST_DATA }}
EnvironmentFile=-/etc/nodectl-services/openlist-initial.env
ExecStart=/usr/local/bin/openlist server --data {{ OPENLIST_DATA }}
Restart=on-failure
RestartSec=3
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
PrivateDevices=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths={{ OPENLIST_DATA }} {{ OPENLIST_FILES }}
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
CapabilityBoundingSet=
# Upstream first initialization prints the password on stdout: never journal it.
StandardOutput=null
StandardError=null

[Install]
WantedBy=multi-user.target
