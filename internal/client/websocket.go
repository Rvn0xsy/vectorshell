package client

import (
	"context"
	"encoding/json"
	"log"
	"net"
	"net/http"
	"net/url"
	"os"
	"runtime"
	"strconv"
	"sync"
	"time"

	"github.com/arch/vectorshell-go/internal/config"
	"github.com/arch/vectorshell-go/internal/protocol"
	"github.com/arch/vectorshell-go/internal/tunnel"
	"github.com/google/uuid"
	"github.com/gorilla/websocket"
)

func Run(ctx context.Context, cfg config.Config) error {
	for {
		if err := runOnce(ctx, cfg); err != nil {
			log.Printf("client disconnected: %v", err)
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(time.Duration(cfg.Client.ReconnectInterval) * time.Second):
		}
	}
}

func runOnce(ctx context.Context, cfg config.Config) error {
	dialer := &websocket.Dialer{
		Proxy:            nil,
		HandshakeTimeout: 10 * time.Second,
	}

	log.Printf("tunnel config: Enabled=%v, ProxyHost=%q, ProxyPort=%d, Host=%q, Port=%d",
		cfg.Tunnel.Enabled, cfg.Tunnel.ProxyHost, cfg.Tunnel.ProxyPort,
		cfg.Tunnel.Host, cfg.Tunnel.Port)

	if cfg.Tunnel.Enabled && cfg.Tunnel.ProxyHost != "" {
		u, err := url.Parse(cfg.Client.ServerURL)
		if err != nil {
			log.Printf("tunnel: failed to parse server URL %s: %v", cfg.Client.ServerURL, err)
			return err
		}
		serverHost, serverPort, err := net.SplitHostPort(u.Host)
		if err != nil {
			// Host may not include a port; default to tunnel port.
			serverHost = u.Host
			serverPort = strconv.Itoa(cfg.Tunnel.Port)
		}
		if cfg.Tunnel.Port > 0 {
			serverPort = strconv.Itoa(cfg.Tunnel.Port)
		}
		dialer.NetDialContext = tunnel.DialFunc(
			cfg.Tunnel.ProxyHost, strconv.Itoa(cfg.Tunnel.ProxyPort),
			serverHost, serverPort,
			[]byte(cfg.Tunnel.PreSharedKey),
		)
		log.Printf("tunnel: dialing via proxy %s:%d -> %s:%s",
			cfg.Tunnel.ProxyHost, cfg.Tunnel.ProxyPort, serverHost, serverPort)
	} else {
		dialer.NetDialContext = (&net.Dialer{
			Timeout:   5 * time.Second,
			KeepAlive: 30 * time.Second,
		}).DialContext
	}
	conn, response, err := dialer.DialContext(ctx, cfg.Client.ServerURL, http.Header{})
	if err != nil {
		if response != nil {
			defer response.Body.Close()
			log.Printf("websocket dial to %s failed with HTTP %s", cfg.Client.ServerURL, response.Status)
		}
		return err
	}
	defer conn.Close()
	log.Printf("connected to %s", cfg.Client.ServerURL)
	var writeMu sync.Mutex
	writeJSON := func(value any) error {
		writeMu.Lock()
		defer writeMu.Unlock()
		return conn.WriteJSON(value)
	}

	reg := protocol.RegisterMessage{
		Token:        cfg.Auth.ClientToken,
		Hostname:     Hostname(),
		OS:           runtime.GOOS,
		Arch:         runtime.GOARCH,
		InstallID:    resolveInstallID(),
		Username:     Username(),
		PID:          os.Getpid(),
		IP:           detectIP(),
		BuildUUID:    uuid.NewString(),
		Timestamp:    time.Now().Unix(),
		Capabilities: []string{"exec", "read_file", "write_file", "upload_file", "download_file"},
	}
	if err := writeJSON(protocol.MustEnvelope("register", uuid.NewString(), reg)); err != nil {
		return err
	}

	go heartbeat(ctx, writeJSON)

	for {
		var env protocol.Envelope
		if err := conn.ReadJSON(&env); err != nil {
			return err
		}
		switch env.Type {
		case "registered":
			continue
		case "tool_call":
			var msg protocol.ToolCallMessage
			if err := json.Unmarshal(env.Payload, &msg); err != nil {
				continue
			}
			timeout := 60 * time.Second
			if msg.TimeoutMS > 0 {
				timeout = time.Duration(msg.TimeoutMS) * time.Millisecond
			}
			callCtx, cancel := context.WithTimeout(ctx, timeout)
			result, err := ExecuteTool(callCtx, msg.ToolName, msg.Args)
			cancel()
			data, _ := json.Marshal(result)
			response := protocol.ToolResultMessage{
				ToolName:   msg.ToolName,
				OK:         err == nil,
				Data:       data,
				DurationMS: timeout.Milliseconds(),
			}
			if err != nil {
				response.Error = err.Error()
			}
			if writeErr := writeJSON(protocol.MustEnvelope("tool_result", env.ID, response)); writeErr != nil {
				return writeErr
			}
		case "ping":
			continue
		}
	}
}

func heartbeat(ctx context.Context, writeJSON func(any) error) {
	ticker := time.NewTicker(15 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			_ = writeJSON(protocol.MustEnvelope("heartbeat", uuid.NewString(), protocol.HeartbeatMessage{Timestamp: time.Now().Unix()}))
		}
	}
}

func resolveInstallID() string {
	if value := os.Getenv("VECTOR_INSTALL_ID"); value != "" {
		return value
	}
	return Hostname() + "-" + runtime.GOOS + "-" + runtime.GOARCH
}

func detectIP() string {
	addrs, err := net.InterfaceAddrs()
	if err != nil {
		return ""
	}
	for _, addr := range addrs {
		ipnet, ok := addr.(*net.IPNet)
		if !ok || ipnet.IP.IsLoopback() {
			continue
		}
		if ipv4 := ipnet.IP.To4(); ipv4 != nil {
			return ipv4.String()
		}
	}
	return ""
}

