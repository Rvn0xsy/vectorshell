package config

import (
	"fmt"
	"os"
	"path/filepath"

	toml "github.com/pelletier/go-toml/v2"
)

type Config struct {
	Server ServerConfig `toml:"server" json:"server"`
	Agent  AgentConfig  `toml:"agent" json:"agent"`
	Client ClientConfig `toml:"client" json:"client"`
	Auth   AuthConfig   `toml:"auth" json:"auth"`
	Skill  SkillConfig  `toml:"skill" json:"skill"`
	Store  StoreConfig  `toml:"store" json:"store"`
	Tunnel TunnelConfig `toml:"tunnel" json:"tunnel"`
}

type ServerConfig struct {
	Listen    string `toml:"listen" json:"listen"`
	WSPath    string `toml:"ws_path" json:"ws_path"`
	UIPath    string `toml:"ui_path" json:"ui_path"`
	UIDist    string `toml:"ui_dist" json:"ui_dist"`
	PublicURL string `toml:"public_url" json:"public_url"`
}

type AgentConfig struct {
	Model    string `toml:"model" json:"model"`
	BaseURL  string `toml:"base_url" json:"base_url"`
	APIKey   string `toml:"api_key" json:"api_key"`
	SoulPath string `toml:"soul_path" json:"soul_path"`
	Prompt   string `toml:"-" json:"-"`
}

type ClientConfig struct {
	ServerURL         string `toml:"server_url" json:"server_url"`
	ReconnectInterval int    `toml:"reconnect_interval" json:"reconnect_interval"`
}

type AuthConfig struct {
	APIToken    string `toml:"api_token" json:"api_token"`
	ClientToken string `toml:"client_token" json:"client_token"`
}

type SkillConfig struct {
	Enabled bool   `toml:"enabled" json:"enabled"`
	Dir     string `toml:"dir" json:"dir"`
}

type StoreConfig struct {
	DBPath string `toml:"db_path" json:"db_path"`
}

type TunnelConfig struct {
	Enabled      bool   `toml:"enabled" json:"enabled"`
	PreSharedKey string `toml:"pre_shared_key" json:"pre_shared_key"`
	Port         int    `toml:"port" json:"port"`
	Host         string `toml:"host" json:"host"`
	ProxyHost    string `toml:"proxy_host" json:"proxy_host"`
	ProxyPort    int    `toml:"proxy_port" json:"proxy_port"`
}

func Default() Config {
	return Config{
		Server: ServerConfig{
			Listen: ":8080",
			WSPath: "/ws",
			UIPath: "/ui",
			UIDist: "dashboard/dist",
		},
		Agent: AgentConfig{
			Model:    "gpt-4.1",
			BaseURL:  "https://api.openai.com/v1",
			SoulPath: "SOUL.md",
			Prompt:   defaultPrompt,
		},
		Client: ClientConfig{
			ServerURL:         "ws://127.0.0.1:8080/ws",
			ReconnectInterval: 5,
		},
		Skill: SkillConfig{
			Enabled: true,
			Dir:     "skill",
		},
		Store: StoreConfig{
			DBPath: "data/vectorshell-go.db",
		},
		Tunnel: TunnelConfig{
			Enabled:      false,
			PreSharedKey: "bypass_proxy_32byte_key!!_abc!!!",
			Port:         7734,
		},
	}
}

func Load(path string) (Config, error) {
	cfg := Default()
	if path == "" {
		return cfg, nil
	}
	configPath, err := filepath.Abs(path)
	if err != nil {
		return Config{}, fmt.Errorf("resolve config path: %w", err)
	}
	configDir := filepath.Dir(configPath)

	data, err := os.ReadFile(configPath)
	if err != nil {
		return Config{}, fmt.Errorf("read config: %w", err)
	}
	if err := toml.Unmarshal(data, &cfg); err != nil {
		return Config{}, fmt.Errorf("parse config: %w", err)
	}
	if cfg.Server.Listen == "" {
		cfg.Server.Listen = ":8080"
	}
	if cfg.Server.WSPath == "" {
		cfg.Server.WSPath = "/ws"
	}
	if cfg.Server.UIPath == "" {
		cfg.Server.UIPath = "/ui"
	}
	if cfg.Server.UIDist == "" {
		cfg.Server.UIDist = "dashboard/dist"
	}
	if !filepath.IsAbs(cfg.Server.UIDist) {
		cfg.Server.UIDist = filepath.Clean(filepath.Join(configDir, cfg.Server.UIDist))
	}
	if cfg.Client.ReconnectInterval <= 0 {
		cfg.Client.ReconnectInterval = 5
	}
	if cfg.Agent.SoulPath == "" {
		cfg.Agent.SoulPath = "SOUL.md"
	}
	if !filepath.IsAbs(cfg.Agent.SoulPath) {
		cfg.Agent.SoulPath = filepath.Clean(filepath.Join(configDir, cfg.Agent.SoulPath))
	}
	if soulData, readErr := os.ReadFile(cfg.Agent.SoulPath); readErr == nil {
		cfg.Agent.Prompt = string(soulData)
	} else if os.IsNotExist(readErr) {
		cfg.Agent.Prompt = defaultPrompt
	} else {
		return Config{}, fmt.Errorf("read soul file: %w", readErr)
	}
	if cfg.Skill.Dir != "" && !filepath.IsAbs(cfg.Skill.Dir) {
		cfg.Skill.Dir = filepath.Clean(filepath.Join(configDir, cfg.Skill.Dir))
	} else if cfg.Skill.Dir == "" {
		cfg.Skill.Dir = filepath.Clean(filepath.Join(configDir, "skill"))
	}
	if cfg.Store.DBPath == "" {
		cfg.Store.DBPath = "data/vectorshell-go.db"
	}
	if !filepath.IsAbs(cfg.Store.DBPath) {
		cfg.Store.DBPath = filepath.Clean(filepath.Join(configDir, cfg.Store.DBPath))
	}
	return cfg, nil
}

const defaultPrompt = `You are VectorShell Agent in Go.
Operate only through the available remote tools.
Prefer read_file and write_file for file operations.
Use exec for shell commands when file tools are insufficient.
If information is missing, ask one concise clarification question.
Keep final answers concise and action-focused.`
