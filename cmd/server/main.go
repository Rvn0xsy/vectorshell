package main

import (
	"flag"
	"log"

	"github.com/arch/vectorshell-go/internal/agent"
	"github.com/arch/vectorshell-go/internal/api"
	"github.com/arch/vectorshell-go/internal/config"
	"github.com/arch/vectorshell-go/internal/session"
	"github.com/arch/vectorshell-go/internal/store"
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
	db, err := store.Open(cfg.Store.DBPath)
	if err != nil {
		log.Fatal(err)
	}
	defer db.Close()
	if err := api.Run(cfg, mgr, agentSvc, db); err != nil {
		log.Fatal(err)
	}
}
