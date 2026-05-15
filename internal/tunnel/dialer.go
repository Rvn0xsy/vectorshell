package tunnel

import (
	"bufio"
	"context"
	"fmt"
	"net"
	"time"
)

// DialFunc returns a function compatible with websocket.Dialer.NetDialContext.
// It establishes a TCP connection through a CONNECT proxy, then wraps it with
// FragmentingConn (1-byte TCP segments for DPI bypass) and EncryptedConn
// (AES-256-CTR).
//
// Parameters proxyHost/proxyPort identify the CONNECT proxy.
// tunnelHost/tunnelPort identify the target tunnel endpoint on the server.
func DialFunc(proxyHost, proxyPort, tunnelHost, tunnelPort string, key []byte) func(ctx context.Context, network, addr string) (net.Conn, error) {
	proxyAddr := net.JoinHostPort(proxyHost, proxyPort)
	tunnelAddr := net.JoinHostPort(tunnelHost, tunnelPort)

	return func(ctx context.Context, network, addr string) (net.Conn, error) {
		dialer := &net.Dialer{Timeout: 10 * time.Second}
		rawConn, err := dialer.DialContext(ctx, "tcp", proxyAddr)
		if err != nil {
			return nil, fmt.Errorf("tunnel: dial proxy %s: %w", proxyAddr, err)
		}

		// HTTP CONNECT to tunnel endpoint
		connectReq := fmt.Sprintf("CONNECT %s HTTP/1.1\r\nHost: %s\r\n\r\n", tunnelAddr, tunnelAddr)
		if _, err := rawConn.Write([]byte(connectReq)); err != nil {
			rawConn.Close()
			return nil, fmt.Errorf("tunnel: CONNECT write: %w", err)
		}

		reader := bufio.NewReader(rawConn)
		_, err = reader.ReadString('\n')
		if err != nil {
			rawConn.Close()
			return nil, fmt.Errorf("tunnel: CONNECT read: %w", err)
		}

		// Drain remaining headers
		for {
			line, err := reader.ReadString('\n')
			if err != nil || line == "\r\n" || line == "\n" {
				break
			}
		}

		// Wrap: fragment (DPI bypass) → encrypt
		fragConn := NewFragmentingConn(rawConn)
		encConn, err := NewEncryptedConn(fragConn, key)
		if err != nil {
			rawConn.Close()
			return nil, fmt.Errorf("tunnel: encryption setup: %w", err)
		}

		return encConn, nil
	}
}
