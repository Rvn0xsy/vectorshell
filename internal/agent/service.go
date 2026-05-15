package agent

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strings"
	"time"

	"github.com/arch/vectorshell-go/internal/config"
	"github.com/arch/vectorshell-go/internal/session"
	"github.com/arch/vectorshell-go/internal/skills"
	openaiext "github.com/cloudwego/eino-ext/components/model/openai"
	"github.com/cloudwego/eino/adk"
	"github.com/cloudwego/eino/components/tool"
	"github.com/cloudwego/eino/compose"
	"github.com/cloudwego/eino/schema"
)

type Service struct {
	cfg config.AgentConfig
	mgr *session.Manager
}

type EventPayload struct {
	Event      string
	InstallID  string
	RequestID  string
	ToolName   string
	Args       map[string]any
	Role       string
	Content    string
	Final      bool
	OK         bool
	DurationMS int64
	Data       any
	Message    string
}

func NewService(cfg config.AgentConfig, mgr *session.Manager) *Service {
	return &Service{cfg: cfg, mgr: mgr}
}

func (s *Service) Run(ctx context.Context, installID, prompt string, skillDir string, enableSkill bool) (string, error) {
	return s.RunWithEvents(ctx, installID, prompt, skillDir, enableSkill, nil)
}

func (s *Service) RunWithEvents(ctx context.Context, installID, prompt string, skillDir string, enableSkill bool, emit func(EventPayload)) (string, error) {
	return s.runWithRetries(ctx, installID, prompt, skillDir, enableSkill, emit)
}

func (s *Service) runWithRetries(ctx context.Context, installID, prompt string, skillDir string, enableSkill bool, emit func(EventPayload)) (string, error) {
	var lastErr error
	for attempt := 0; attempt < 4; attempt++ {
		answer, err := s.runOnce(ctx, installID, prompt, skillDir, enableSkill, emit)
		if err == nil {
			if emit != nil {
				emit(EventPayload{Event: "agent.message", InstallID: installID, Role: string(schema.Assistant), Content: answer, Final: true})
			}
			return answer, nil
		}
		lastErr = err
		if !shouldRetryAgentError(err) || attempt == 3 {
			return "", err
		}
		select {
		case <-ctx.Done():
			return "", ctx.Err()
		case <-time.After(time.Duration(1<<attempt) * time.Second):
		}
	}
	if lastErr != nil {
		return "", lastErr
	}
	return "", fmt.Errorf("agent returned empty response")
}

func (s *Service) runOnce(ctx context.Context, installID, prompt string, skillDir string, enableSkill bool, emit func(EventPayload)) (string, error) {
	model, err := openaiext.NewChatModel(ctx, &openaiext.ChatModelConfig{
		APIKey:  firstNonEmpty(s.cfg.APIKey, os.Getenv("OPENAI_API_KEY")),
		Model:   firstNonEmpty(s.cfg.Model, os.Getenv("OPENAI_MODEL")),
		BaseURL: normalizeBaseURL(firstNonEmpty(s.cfg.BaseURL, os.Getenv("OPENAI_BASE_URL"))),
		ByAzure: os.Getenv("OPENAI_BY_AZURE") == "true",
	})
	if err != nil {
		return "", fmt.Errorf("create chat model: %w", err)
	}

	handlers := make([]adk.ChatModelAgentMiddleware, 0, 1)
	if enableSkill {
		skillMiddleware, err := skills.MiddlewareFromDir(ctx, skillDir)
		if err != nil {
			return "", fmt.Errorf("load skills: %w", err)
		}
		if skillMiddleware != nil {
			handlers = append(handlers, skillMiddleware)
		}
	}

	agentTools := []tool.BaseTool{
		newRemoteTool(s.mgr, installID, "exec", "Execute a shell command on the selected client.", map[string]*schema.ParameterInfo{
			"command": {Type: "string", Desc: "Shell command to execute", Required: true},
		}, emit),
		newRemoteTool(s.mgr, installID, "read_file", "Read a file from the selected client.", map[string]*schema.ParameterInfo{
			"path": {Type: "string", Desc: "Client-side path to read", Required: true},
		}, emit),
		newRemoteTool(s.mgr, installID, "write_file", "Write a file on the selected client.", map[string]*schema.ParameterInfo{
			"path":    {Type: "string", Desc: "Client-side path to write", Required: true},
			"content": {Type: "string", Desc: "Text content to write", Required: true},
		}, emit),
		newCustomRemoteTool(s.mgr, installID, "upload_file", "Upload a local server file to the selected client.", map[string]*schema.ParameterInfo{
			"src": {Type: "string", Desc: "Server-local source path", Required: true},
			"dst": {Type: "string", Desc: "Client-side destination path", Required: true},
		}, func(ctx context.Context, args map[string]any) (string, error) {
			startedAt := time.Now()
			emitToolStart(emit, installID, "upload_file", args)
			src, _ := args["src"].(string)
			dst, _ := args["dst"].(string)
			result, err := s.mgr.UploadFile(ctx, installID, src, dst, 60*time.Second)
			if err != nil {
				emitToolFinish(emit, installID, "upload_file", time.Since(startedAt).Milliseconds(), false, nil, err.Error())
				return "", err
			}
			emitToolFinish(emit, installID, "upload_file", time.Since(startedAt).Milliseconds(), true, json.RawMessage(result), "")
			return string(result), nil
		}),
		newCustomRemoteTool(s.mgr, installID, "download_file", "Download a client file to the server.", map[string]*schema.ParameterInfo{
			"src": {Type: "string", Desc: "Client-side source path", Required: true},
			"dst": {Type: "string", Desc: "Server-local destination path", Required: true},
		}, func(ctx context.Context, args map[string]any) (string, error) {
			startedAt := time.Now()
			emitToolStart(emit, installID, "download_file", args)
			src, _ := args["src"].(string)
			dst, _ := args["dst"].(string)
			result, err := s.mgr.DownloadFile(ctx, installID, src, dst, 60*time.Second)
			if err != nil {
				emitToolFinish(emit, installID, "download_file", time.Since(startedAt).Milliseconds(), false, nil, err.Error())
				return "", err
			}
			emitToolFinish(emit, installID, "download_file", time.Since(startedAt).Milliseconds(), true, json.RawMessage(result), "")
			return string(result), nil
		}),
	}

	a, err := adk.NewChatModelAgent(ctx, &adk.ChatModelAgentConfig{
		Name:        "vectorshell_agent",
		Description: "An AI-driven remote command execution agent implemented in Go.",
		Instruction: s.cfg.Prompt + "\nTarget install_id: " + installID,
		Model:       model,
		Handlers:    handlers,
		ToolsConfig: adk.ToolsConfig{
			ToolsNodeConfig: compose.ToolsNodeConfig{Tools: agentTools},
		},
		MaxIterations: 20,
	})
	if err != nil {
		return "", fmt.Errorf("create agent: %w", err)
	}

	runner := adk.NewRunner(ctx, adk.RunnerConfig{Agent: a, EnableStreaming: false})
	iter := runner.Query(ctx, prompt)
	var chunks []string
	for {
		event, ok := iter.Next()
		if !ok {
			break
		}
		if event.Err != nil {
			return "", event.Err
		}
		if event.Output == nil || event.Output.MessageOutput == nil {
			continue
		}
		msg, content, ok := extractMessageContent(event)
		if ok && msg != nil && msg.Role != schema.Tool && strings.TrimSpace(content) != "" {
			chunks = append(chunks, content)
		}
	}
	answer := strings.TrimSpace(strings.Join(chunks, ""))
	if answer == "" {
		return "", fmt.Errorf("agent returned empty response")
	}
	return answer, nil
}

type remoteTool struct {
	mgr         *session.Manager
	installID   string
	name        string
	description string
	params      map[string]*schema.ParameterInfo
	run         func(ctx context.Context, args map[string]any) (string, error)
	emit        func(EventPayload)
}

func newRemoteTool(mgr *session.Manager, installID, name, description string, params map[string]*schema.ParameterInfo, emit func(EventPayload)) tool.InvokableTool {
	return &remoteTool{mgr: mgr, installID: installID, name: name, description: description, params: params, emit: emit}
}

func newCustomRemoteTool(mgr *session.Manager, installID, name, description string, params map[string]*schema.ParameterInfo, run func(ctx context.Context, args map[string]any) (string, error)) tool.InvokableTool {
	return &remoteTool{mgr: mgr, installID: installID, name: name, description: description, params: params, run: run}
}

func (t *remoteTool) Info(ctx context.Context) (*schema.ToolInfo, error) {
	return &schema.ToolInfo{
		Name:        t.name,
		Desc:        t.description,
		ParamsOneOf: schema.NewParamsOneOfByParams(t.params),
	}, nil
}

func (t *remoteTool) InvokableRun(ctx context.Context, argumentsInJSON string, opts ...tool.Option) (string, error) {
	var args map[string]any
	if err := json.Unmarshal([]byte(argumentsInJSON), &args); err != nil {
		return "", err
	}
	startedAt := time.Now()
	emitToolStart(t.emit, t.installID, t.name, args)
	if t.run != nil {
		result, err := t.run(ctx, args)
		if err != nil {
			emitToolFinish(t.emit, t.installID, t.name, time.Since(startedAt).Milliseconds(), false, nil, err.Error())
			return "", err
		}
		emitToolFinish(t.emit, t.installID, t.name, time.Since(startedAt).Milliseconds(), true, result, "")
		return result, nil
	}
	result, err := t.mgr.DispatchTool(ctx, t.installID, t.name, args, 60*time.Second)
	if err != nil {
		emitToolFinish(t.emit, t.installID, t.name, time.Since(startedAt).Milliseconds(), false, nil, err.Error())
		return "", err
	}
	emitToolFinish(t.emit, t.installID, t.name, time.Since(startedAt).Milliseconds(), true, json.RawMessage(result), "")
	return string(result), nil
}

func emitToolStart(emit func(EventPayload), installID, toolName string, args map[string]any) {
	if emit == nil {
		return
	}
	emit(EventPayload{Event: "tool.started", InstallID: installID, ToolName: toolName, Args: args})
}

func emitToolFinish(emit func(EventPayload), installID, toolName string, durationMS int64, ok bool, data any, message string) {
	if emit == nil {
		return
	}
	emit(EventPayload{Event: "tool.finished", InstallID: installID, ToolName: toolName, DurationMS: durationMS, OK: ok, Data: data, Message: message})
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return value
		}
	}
	return ""
}

func normalizeBaseURL(value string) string {
	trimmed := strings.TrimSpace(value)
	if trimmed == "" {
		return ""
	}
	trimmed = strings.TrimRight(trimmed, "/")
	if strings.HasSuffix(trimmed, "/v1") {
		return trimmed
	}
	return trimmed + "/v1"
}

func shouldRetryAgentError(err error) bool {
	if err == nil {
		return false
	}
	message := strings.ToLower(err.Error())
	for _, marker := range []string{"429", "503", "resourceexhausted", "too many requests", "service unavailable", "cooling down", "busy"} {
		if strings.Contains(message, marker) {
			return true
		}
	}
	return false
}

func extractMessageContent(event *adk.AgentEvent) (*schema.Message, string, bool) {
	if event == nil || event.Output == nil || event.Output.MessageOutput == nil {
		return nil, "", false
	}
	if msg, _, err := adk.GetMessage(event); err == nil && msg != nil {
		return msg, msg.Content, true
	}
	mo := event.Output.MessageOutput
	if mo.Message != nil {
		return mo.Message, mo.Message.Content, true
	}
	if mo.IsStreaming && mo.MessageStream != nil {
		var builder strings.Builder
		var lastRole schema.RoleType
		for {
			chunk, err := mo.MessageStream.Recv()
			if err != nil {
				if err == io.EOF {
					break
				}
				return nil, "", false
			}
			if chunk != nil {
				lastRole = chunk.Role
				builder.WriteString(chunk.Content)
			}
		}
		content := builder.String()
		if strings.TrimSpace(content) == "" {
			return nil, "", false
		}
		return &schema.Message{Role: lastRole, Content: content}, content, true
	}
	return nil, "", false
}
