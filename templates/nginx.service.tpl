[Unit]
Description=Nginx managed by nodectl
After=network-online.target
Wants=network-online.target

[Service]
Type=forking
PIDFile=/run/nginx.pid
ExecStartPre={{ NGINX_BINARY }} -t -c /etc/nginx/nginx.conf
ExecStart={{ NGINX_BINARY }} -c /etc/nginx/nginx.conf
ExecReload={{ NGINX_BINARY }} -s reload -c /etc/nginx/nginx.conf
ExecStop={{ NGINX_BINARY }} -s quit -c /etc/nginx/nginx.conf
PrivateTmp=false
Restart=on-failure

[Install]
WantedBy=multi-user.target
