package client

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

type ExecResult struct {
	Command    string            `json:"command"`
	Stdout     string            `json:"stdout"`
	Stderr     string            `json:"stderr"`
	ExitCode   int               `json:"exit_code"`
	DurationMS int64             `json:"duration_ms"`
	CWD        string            `json:"cwd"`
	Env        map[string]string `json:"env,omitempty"`
}

func RunCommand(ctx context.Context, command string) ExecResult {
	start := time.Now()
	cmd := exec.CommandContext(ctx, shellCommand(runtime.GOOS), shellArgs(runtime.GOOS, command)...)
	var stdout bytes.Buffer
	var stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	cwd, _ := os.Getwd()
	cmd.Dir = cwd
	err := cmd.Run()
	exitCode := 0
	if err != nil {
		exitCode = 1
		if exitErr, ok := err.(*exec.ExitError); ok {
			exitCode = exitErr.ExitCode()
		}
	}
	return ExecResult{
		Command:    command,
		Stdout:     stdout.String(),
		Stderr:     stderr.String(),
		ExitCode:   exitCode,
		DurationMS: time.Since(start).Milliseconds(),
		CWD:        cwd,
		Env: map[string]string{
			"GOOS":   runtime.GOOS,
			"GOARCH": runtime.GOARCH,
		},
	}
}

func shellCommand(goos string) string {
	if goos == "windows" {
		return "cmd"
	}
	return "/bin/sh"
}

func shellArgs(goos, command string) []string {
	if goos == "windows" {
		return []string{"/c", command}
	}
	return []string{"-lc", command}
}

func ExecuteTool(ctx context.Context, toolName string, args json.RawMessage) (any, error) {
	switch toolName {
	case "exec":
		var payload struct {
			Command string `json:"command"`
		}
		if err := json.Unmarshal(args, &payload); err != nil {
			return nil, err
		}
		result := RunCommand(ctx, payload.Command)
		return result, nil
	case "read_file":
		var payload struct {
			Path string `json:"path"`
		}
		if err := json.Unmarshal(args, &payload); err != nil {
			return nil, err
		}
		data, err := os.ReadFile(payload.Path)
		if err != nil {
			return nil, err
		}
		return map[string]any{"path": payload.Path, "content": string(data)}, nil
	case "write_file":
		var payload struct {
			Path    string `json:"path"`
			Content string `json:"content"`
		}
		if err := json.Unmarshal(args, &payload); err != nil {
			return nil, err
		}
		if err := os.MkdirAll(filepath.Dir(payload.Path), 0o755); err != nil {
			return nil, err
		}
		if err := os.WriteFile(payload.Path, []byte(payload.Content), 0o644); err != nil {
			return nil, err
		}
		return map[string]any{"path": payload.Path, "written": len(payload.Content)}, nil
	case "upload_file":
		var payload struct {
			Src           string `json:"src"`
			Dst           string `json:"dst"`
			ContentBase64 string `json:"content_base64"`
			Append        bool   `json:"append"`
		}
		if err := json.Unmarshal(args, &payload); err != nil {
			return nil, err
		}
		if payload.Dst == "" {
			payload.Dst = payload.Src
		}
		if payload.ContentBase64 == "" && payload.Src != "" {
			data, err := os.ReadFile(payload.Src)
			if err != nil {
				return nil, err
			}
			payload.ContentBase64 = base64.StdEncoding.EncodeToString(data)
		}
		decoded, err := base64.StdEncoding.DecodeString(payload.ContentBase64)
		if err != nil {
			return nil, err
		}
		if err := os.MkdirAll(filepath.Dir(payload.Dst), 0o755); err != nil {
			return nil, err
		}
		flag := os.O_CREATE | os.O_WRONLY
		if payload.Append {
			flag |= os.O_APPEND
		} else {
			flag |= os.O_TRUNC
		}
		file, err := os.OpenFile(payload.Dst, flag, 0o644)
		if err != nil {
			return nil, err
		}
		defer file.Close()
		if _, err := file.Write(decoded); err != nil {
			return nil, err
		}
		return map[string]any{"src": payload.Src, "dst": payload.Dst, "bytes": len(decoded)}, nil
	case "download_file":
		var payload struct {
			Src string `json:"src"`
		}
		if err := json.Unmarshal(args, &payload); err != nil {
			return nil, err
		}
		data, err := os.ReadFile(payload.Src)
		if err != nil {
			return nil, err
		}
		return map[string]any{
			"src":            payload.Src,
			"content_base64": base64.StdEncoding.EncodeToString(data),
			"bytes":          len(data),
		}, nil
	default:
		return nil, fmt.Errorf("unsupported tool: %s", toolName)
	}
}

func Hostname() string {
	name, _ := os.Hostname()
	return name
}

func Username() string {
	if value := strings.TrimSpace(os.Getenv("USER")); value != "" {
		return value
	}
	return strings.TrimSpace(os.Getenv("USERNAME"))
}
