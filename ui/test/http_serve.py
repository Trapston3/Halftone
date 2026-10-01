import http.server, socketserver, sys, os
os.chdir(os.path.dirname(os.path.abspath(__file__))+"/..")   # serve ui/
port=int(sys.argv[1]) if len(sys.argv)>1 else 8765
class H(http.server.SimpleHTTPRequestHandler):
    def log_message(self,*a): pass
    def end_headers(self):
        self.send_header("Cache-Control","no-store")
        super().end_headers()
socketserver.TCPServer.allow_reuse_address=True
with socketserver.TCPServer(("127.0.0.1",port),H) as httpd:
    print("serving ui/ on",port,flush=True)
    httpd.serve_forever()
