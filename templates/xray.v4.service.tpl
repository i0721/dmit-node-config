[Unit]
Description=Xray Core managed by nodectl
After=network-online.target
Wants=network-online.target
{{ CN_UNIT }}

[Service]
Type=simple
User={{ XRAY_USER }}
# The primary socket group must be the Nginx group; supplementary groups alone
# do not change the group owner of newly created Unix sockets.
Group={{ NGINX_USER }}
LoadCredential=xray.json:/etc/xray/config.json
{{ CN_ASSETS }}
ExecStartPre={{ XRAY_BINARY }} run -test -config %d/xray.json
ExecStart={{ XRAY_BINARY }} run -config %d/xray.json
Restart=on-failure
RestartSec=3
UMask=0007
LimitNOFILE=1048576
NoNewPrivileges=true
PrivateTmp=false

[Install]
WantedBy=multi-user.target
