// Package tunnel provides connection wrappers for evading DPI on CONNECT
// proxy connections. TCP fragmentation splits writes into 1-byte segments
// to defeat DPI reassembly; AES-256-CTR encryption prevents content inspection.
package tunnel

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"io"
	"net"
	"sync"
)

// ---- FragmentingConn ----

// FragmentingConn wraps a net.Conn and fragments every Write into 1-byte
// TCP segments with no delay, to defeat DPI reassembly.
type FragmentingConn struct {
	net.Conn
	mu sync.Mutex
}

// NewFragmentingConn returns a connection that fragments all writes.
func NewFragmentingConn(conn net.Conn) *FragmentingConn {
	return &FragmentingConn{Conn: conn}
}

func (c *FragmentingConn) Write(b []byte) (int, error) {
	c.mu.Lock()
	defer c.mu.Unlock()

	for i := 0; i < len(b); i++ {
		if _, err := c.Conn.Write(b[i : i+1]); err != nil {
			return i, err
		}
	}
	return len(b), nil
}

// ---- EncryptedConn ----

// EncryptedConn wraps a net.Conn with AES-256-CTR encryption.
// On creation, both sides exchange random IVs over the underlying connection.
type EncryptedConn struct {
	net.Conn
	writeStream cipher.Stream
	readStream  cipher.Stream
	writeBuf    []byte
	readBuf     []byte
}

// NewEncryptedConn performs the IV handshake and returns an encrypted connection.
// Both sides must call this with the same key.
func NewEncryptedConn(conn net.Conn, key []byte) (*EncryptedConn, error) {
	writeIV := make([]byte, aes.BlockSize)
	if _, err := rand.Read(writeIV); err != nil {
		return nil, err
	}

	if _, err := conn.Write(writeIV); err != nil {
		return nil, err
	}

	readIV := make([]byte, aes.BlockSize)
	if _, err := io.ReadFull(conn, readIV); err != nil {
		return nil, err
	}

	writeBlock, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	readBlock, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}

	return &EncryptedConn{
		Conn:        conn,
		writeStream: cipher.NewCTR(writeBlock, writeIV),
		readStream:  cipher.NewCTR(readBlock, readIV),
	}, nil
}

func (c *EncryptedConn) Write(b []byte) (int, error) {
	c.writeBuf = growBuf(c.writeBuf, len(b))
	c.writeStream.XORKeyStream(c.writeBuf[:len(b)], b)
	return c.Conn.Write(c.writeBuf[:len(b)])
}

func (c *EncryptedConn) Read(b []byte) (int, error) {
	n, err := c.Conn.Read(b)
	if n > 0 {
		c.readBuf = growBuf(c.readBuf, n)
		c.readStream.XORKeyStream(b[:n], b[:n])
	}
	return n, err
}

func growBuf(buf []byte, size int) []byte {
	if cap(buf) >= size {
		return buf[:size]
	}
	return make([]byte, size)
}
