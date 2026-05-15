package main

import (
	"context"
	"flag"
	"log"
	"strconv"

	clientpkg "github.com/arch/vectorshell-go/internal/client"
	"github.com/arch/vectorshell-go/internal/config"
	"github.com/arch/vectorshell-go/internal/embedded"
)

func main() {
	configPath := flag.String("config", "", "Path to TOML config")
	flag.Parse()

	cfg, err := config.Load(*configPath)
	if err != nil {
		if *configPath == "" {
			cfg = config.Default()
		} else {
			log.Fatal(err)
		}
	}
	if embedded.ServerURL != "" {
		cfg.Client.ServerURL = embedded.ServerURL
	}
	if embedded.ClientToken != "" {
		cfg.Auth.ClientToken = embedded.ClientToken
	}
	if embedded.ReconnectInterval != "" {
		if seconds, parseErr := strconv.Atoi(embedded.ReconnectInterval); parseErr == nil && seconds > 0 {
			cfg.Client.ReconnectInterval = seconds
		}
	}
	if embedded.TunnelEnabled == "true" {
		cfg.Tunnel.Enabled = true
	}
	if embedded.TunnelPSK != "" {
		cfg.Tunnel.PreSharedKey = embedded.TunnelPSK
	}
	if embedded.TunnelPort != "" {
		if port, parseErr := strconv.Atoi(embedded.TunnelPort); parseErr == nil && port > 0 {
			cfg.Tunnel.Port = port
		}
	}
	if embedded.TunnelHost != "" {
		cfg.Tunnel.Host = embedded.TunnelHost
	}
	if embedded.TunnelProxyHost != "" {
		cfg.Tunnel.ProxyHost = embedded.TunnelProxyHost
	}
	if embedded.TunnelProxyPort != "" {
		if port, parseErr := strconv.Atoi(embedded.TunnelProxyPort); parseErr == nil && port > 0 {
			cfg.Tunnel.ProxyPort = port
		}
	}
	if err := clientpkg.Run(context.Background(), cfg); err != nil {
		log.Fatal(err)
	}
}
