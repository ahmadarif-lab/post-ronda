#!/usr/bin/env python3
"""Offline echo server for trying Post Ronda. Listens on loopback only."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json

class Handler(BaseHTTPRequestHandler):
    def respond(self):
        size = int(self.headers.get('Content-Length', 0))
        if size > 10 * 1024 * 1024:
            self.send_error(413)
            return
        if self.path.split('?')[0] == '/download':
            # A binary file response, for trying "Save response to a file".
            payload = bytes(range(256)) * 20
            self.send_response(200)
            self.send_header('Content-Type', 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet')
            self.send_header('Content-Disposition', 'attachment; filename="Laporan Mei.xlsx"')
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        body = self.rfile.read(size).decode('utf-8', errors='replace')
        data = {'message': 'Hello from Post Ronda', 'method': self.command,
                'path': self.path, 'headers': dict(self.headers), 'body': body}
        if self.path.split('?')[0] == '/health':
            data = {'message': 'Check API', 'data': True}
        encoded = json.dumps(data, indent=2).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)
    do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = do_OPTIONS = respond

if __name__ == '__main__':
    print('Post Ronda demo: http://127.0.0.1:9417', flush=True)
    ThreadingHTTPServer(('127.0.0.1', 9417), Handler).serve_forever()
