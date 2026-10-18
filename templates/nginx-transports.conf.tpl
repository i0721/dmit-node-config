        location ^~ {{ XHTTP_PATH }} {
            client_max_body_size 0;
            client_body_timeout 1h;
            grpc_read_timeout 1h;
            grpc_send_timeout 1h;
            grpc_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
            grpc_set_header X-Real-IP $remote_addr;
            grpc_pass unix:{{ XHTTP_SOCKET }};
        }
        location = {{ WS_PATH }} {
            proxy_http_version 1.1;
            proxy_set_header Upgrade $http_upgrade;
            proxy_set_header Connection "upgrade";
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto $scheme;
            proxy_read_timeout 1h;
            proxy_send_timeout 1h;
            proxy_buffering off;
            proxy_request_buffering off;
            proxy_pass http://unix:{{ WS_SOCKET }}:;
        }
