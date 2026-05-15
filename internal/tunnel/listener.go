package tunnel

import (
	"log"
	"net"
)

// Listener wraps a net.Listener and performs AES key exchange on each
// accepted connection, returning an EncryptedConn.
type Listener struct {
	net.Listener
	key []byte
}

// NewListener creates a tunnel listener that unwraps encrypted connections.
// Accepted connections go through the AES-256-CTR IV handshake automatically.
func NewListener(inner net.Listener, key []byte) *Listener {
	return &Listener{Listener: inner, key: key}
}

// Accept waits for the next connection, performs the IV handshake,
// and returns the encrypted connection.
func (l *Listener) Accept() (net.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}

	encConn, err := NewEncryptedConn(conn, l.key)
	if err != nil {
		conn.Close()
		log.Printf("tunnel: encryption handshake failed from %s: %v", conn.RemoteAddr(), err)
		return nil, err
	}

	log.Printf("tunnel: encrypted connection from %s", conn.RemoteAddr())
	return encConn, nil
}
