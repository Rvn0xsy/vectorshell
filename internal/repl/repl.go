package repl

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/arch/vectorshell-go/internal/agent"
	"github.com/arch/vectorshell-go/internal/session"
)

type Runner struct {
	mgr   *session.Manager
	agent *agent.Service
}

func NewRunner(mgr *session.Manager, agentSvc *agent.Service) *Runner {
	return &Runner{mgr: mgr, agent: agentSvc}
}

func (r *Runner) Run(ctx context.Context) error {
	reader := bufio.NewReader(os.Stdin)
	selected := ""
	for {
		fmt.Print("vectorshell> ")
		line, err := reader.ReadString('\n')
		if err != nil {
			return err
		}
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		if line == "/quit" || line == "/exit" {
			return nil
		}
		if line == "/sessions" {
			for _, sess := range r.mgr.List() {
				fmt.Printf("%s\t%s\t%s/%s\t%s\n", sess.InstallID, sess.Hostname, sess.OS, sess.Arch, sess.LastSeen.Format(time.RFC3339))
			}
			continue
		}
		if strings.HasPrefix(line, "/use ") {
			selected = strings.TrimSpace(strings.TrimPrefix(line, "/use "))
			fmt.Printf("selected %s\n", selected)
			continue
		}
		if line == "/back" {
			selected = ""
			fmt.Println("selection cleared")
			continue
		}
		if strings.HasPrefix(line, "/exec ") {
			if selected == "" {
				fmt.Println("no install_id selected")
				continue
			}
			command := strings.TrimSpace(strings.TrimPrefix(line, "/exec "))
			result, err := r.mgr.DispatchTool(ctx, selected, "exec", map[string]any{"command": command}, 60*time.Second)
			if err != nil {
				fmt.Println("error:", err)
				continue
			}
			fmt.Println(string(result))
			continue
		}
		if strings.HasPrefix(line, "/tool ") {
			if selected == "" {
				fmt.Println("no install_id selected")
				continue
			}
			rest := strings.TrimSpace(strings.TrimPrefix(line, "/tool "))
			parts := strings.SplitN(rest, " ", 2)
			if len(parts) != 2 {
				fmt.Println("usage: /tool <name> <json>")
				continue
			}
			var args map[string]any
			if err := json.Unmarshal([]byte(parts[1]), &args); err != nil {
				fmt.Println("invalid json:", err)
				continue
			}
			result, err := r.mgr.DispatchTool(ctx, selected, parts[0], args, 60*time.Second)
			if err != nil {
				fmt.Println("error:", err)
				continue
			}
			fmt.Println(string(result))
			continue
		}
		if strings.HasPrefix(line, "/agent ") {
			if selected == "" {
				fmt.Println("no install_id selected")
				continue
			}
			prompt := strings.TrimSpace(strings.TrimPrefix(line, "/agent "))
			answer, err := r.agent.Run(ctx, selected, prompt, "skill", true)
			if err != nil {
				fmt.Println("error:", err)
				continue
			}
			fmt.Println(answer)
			continue
		}
		fmt.Println("commands: /sessions /use <install_id> /exec <cmd> /tool <name> <json> /agent <prompt> /back /quit")
	}
}
