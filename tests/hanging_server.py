import socket

# Accepts connections but never answers, so a readiness probe against this
# port blocks for the full configured timeout. Used to check that Ctrl+C is
# still honoured while a probe is in flight.
server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", 8130))
server.listen(5)

accepted = []
while True:
    connection, _ = server.accept()
    accepted.append(connection)
