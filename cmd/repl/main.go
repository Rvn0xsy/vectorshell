package main

import (
	"context"
	"flag"
	"log"

	"github.com/arch/vectorshell-go/internal/agent"
	"github.com/arch/vectorshell-go/internal/config"
	"github.com/arch/vectorshell-go/internal/repl"
	"github.com/arch/vectorshell-go/internal/session"
)

func main() {
	configPath := flag.String("config", "", "Path to TOML config")
	flag.Parse()

	cfg, err := config.Load(*configPath)
	if err != nil {
		log.Fatal(err)
	}
	mgr := session.NewManager()
	agentSvc := agent.NewService(cfg.Agent, mgr)
	runner := repl.NewRunner(mgr, agentSvc)
	if err := runner.Run(context.Background()); err != nil {
		log.Fatal(err)
	}
}
