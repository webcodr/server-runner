from http.server import BaseHTTPRequestHandler, HTTPServer


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/ready":
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"OK")
            return

        self.send_response(302)
        self.send_header("Location", "http://127.0.0.1:8125/ready")
        self.end_headers()

    def log_message(self, format, *args):
        pass


HTTPServer(("127.0.0.1", 8125), Handler).serve_forever()
