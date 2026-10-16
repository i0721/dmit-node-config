import http.server,ssl
server=http.server.ThreadingHTTPServer(('127.0.0.1',9443),http.server.SimpleHTTPRequestHandler)
context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain('/etc/ssl/dmit/domain/fullchain.pem','/etc/ssl/dmit/domain/privkey.pem')
server.socket=context.wrap_socket(server.socket,server_side=True)
server.serve_forever()
