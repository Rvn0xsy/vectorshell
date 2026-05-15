package session

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/arch/vectorshell-go/internal/protocol"
	"github.com/google/uuid"
	"github.com/gorilla/websocket"
)

var ErrSessionNotFound = errors.New("session not found")

type ClientSession struct {
	SessionID    string                     `json:"session_id"`
	InstallID    string                     `json:"install_id"`
	Hostname     string                     `json:"hostname"`
	OS           string                     `json:"os"`
	Arch         string                     `json:"arch"`
	Username     string                     `json:"username"`
	PID          int                        `json:"pid"`
	Capabilities []string                   `json:"capabilities"`
	LastSeen     time.Time                  `json:"last_seen"`
	Conn         *websocket.Conn            `json:"-"`
	Pending      map[string]chan toolResult `json:"-"`
	Mu           sync.Mutex                 `json:"-"`
}

type SessionInfo struct {
	SessionID    string    `json:"session_id"`
	InstallID    string    `json:"install_id"`
	Hostname     string    `json:"hostname"`
	OS           string    `json:"os"`
	Arch         string    `json:"arch"`
	Username     string    `json:"username"`
	PID          int       `json:"pid"`
	Capabilities []string  `json:"capabilities"`
	LastSeen     time.Time `json:"last_seen"`
}

type toolResult struct {
	Message protocol.ToolResultMessage
	Err     error
}

type Manager struct {
	mu        sync.RWMutex
	sessions  map[string]*ClientSession
	byInstall map[string]string
}

func NewManager() *Manager {
	return &Manager{
		sessions:  make(map[string]*ClientSession),
		byInstall: make(map[string]string),
	}
}

func (m *Manager) Add(conn *websocket.Conn, reg protocol.RegisterMessage) *ClientSession {
	sessionID := uuid.NewString()
	s := &ClientSession{
		SessionID:    sessionID,
		InstallID:    reg.InstallID,
		Hostname:     reg.Hostname,
		OS:           reg.OS,
		Arch:         reg.Arch,
		Username:     reg.Username,
		PID:          reg.PID,
		Capabilities: append([]string(nil), reg.Capabilities...),
		LastSeen:     time.Now(),
		Conn:         conn,
		Pending:      make(map[string]chan toolResult),
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	m.sessions[sessionID] = s
	m.byInstall[reg.InstallID] = sessionID
	return s
}

func (m *Manager) Remove(sessionID string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	s, ok := m.sessions[sessionID]
	if !ok {
		return
	}
	delete(m.byInstall, s.InstallID)
	delete(m.sessions, sessionID)
}

func (m *Manager) Touch(sessionID string) {
	m.mu.RLock()
	s := m.sessions[sessionID]
	m.mu.RUnlock()
	if s == nil {
		return
	}
	s.Mu.Lock()
	s.LastSeen = time.Now()
	s.Mu.Unlock()
}

func (m *Manager) List() []SessionInfo {
	m.mu.RLock()
	defer m.mu.RUnlock()
	out := make([]SessionInfo, 0, len(m.sessions))
	for _, s := range m.sessions {
		s.Mu.Lock()
		info := SessionInfo{
			SessionID:    s.SessionID,
			InstallID:    s.InstallID,
			Hostname:     s.Hostname,
			OS:           s.OS,
			Arch:         s.Arch,
			Username:     s.Username,
			PID:          s.PID,
			Capabilities: append([]string(nil), s.Capabilities...),
			LastSeen:     s.LastSeen,
		}
		s.Mu.Unlock()
		out = append(out, info)
	}
	return out
}

func (m *Manager) GetByInstallID(installID string) *ClientSession {
	m.mu.RLock()
	defer m.mu.RUnlock()
	sessionID := m.byInstall[installID]
	return m.sessions[sessionID]
}

func (m *Manager) GetBySessionID(sessionID string) *ClientSession {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.sessions[sessionID]
}

func (m *Manager) DispatchTool(ctx context.Context, installID, toolName string, args any, timeout time.Duration) (json.RawMessage, error) {
	s := m.GetByInstallID(installID)
	if s == nil {
		return nil, ErrSessionNotFound
	}

	argBytes, err := json.Marshal(args)
	if err != nil {
		return nil, fmt.Errorf("marshal tool args: %w", err)
	}

	requestID := uuid.NewString()
	env := protocol.MustEnvelope("tool_call", requestID, protocol.ToolCallMessage{
		ToolName:  toolName,
		Args:      argBytes,
		TimeoutMS: timeout.Milliseconds(),
	})

	s.Mu.Lock()
	writeErr := s.Conn.WriteJSON(env)
	s.Mu.Unlock()
	if writeErr != nil {
		return nil, fmt.Errorf("send tool call: %w", writeErr)
	}

	result, err := m.AwaitToolResult(ctx, installID, requestID)
	if err != nil {
		return nil, err
	}
	if !result.OK {
		return nil, errors.New(result.Error)
	}
	return result.Data, nil
}

func (m *Manager) UploadFile(ctx context.Context, installID, src, dst string, timeout time.Duration) (json.RawMessage, error) {
	data, err := os.ReadFile(src)
	if err != nil {
		return nil, fmt.Errorf("read local file: %w", err)
	}
	return m.DispatchTool(ctx, installID, "upload_file", map[string]any{
		"src":            src,
		"dst":            dst,
		"content_base64": base64.StdEncoding.EncodeToString(data),
		"append":         false,
	}, timeout)
}

func (m *Manager) DownloadFile(ctx context.Context, installID, src, dst string, timeout time.Duration) (json.RawMessage, error) {
	result, err := m.DispatchTool(ctx, installID, "download_file", map[string]any{
		"src": src,
	}, timeout)
	if err != nil {
		return nil, err
	}
	var payload struct {
		Src           string `json:"src"`
		ContentBase64 string `json:"content_base64"`
		Bytes         int    `json:"bytes"`
	}
	if err := json.Unmarshal(result, &payload); err != nil {
		return nil, fmt.Errorf("decode download response: %w", err)
	}
	decoded, err := base64.StdEncoding.DecodeString(payload.ContentBase64)
	if err != nil {
		return nil, fmt.Errorf("decode downloaded content: %w", err)
	}
	if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
		return nil, fmt.Errorf("create local directory: %w", err)
	}
	if err := os.WriteFile(dst, decoded, 0o644); err != nil {
		return nil, fmt.Errorf("write local file: %w", err)
	}
	response, err := json.Marshal(map[string]any{
		"src":   payload.Src,
		"dst":   dst,
		"bytes": len(decoded),
	})
	if err != nil {
		return nil, err
	}
	return response, nil
}

func (m *Manager) AwaitToolResult(ctx context.Context, installID, requestID string) (protocol.ToolResultMessage, error) {
	s := m.GetByInstallID(installID)
	if s == nil {
		return protocol.ToolResultMessage{}, context.Canceled
	}
	ch := make(chan toolResult, 1)
	s.Mu.Lock()
	s.Pending[requestID] = ch
	s.Mu.Unlock()
	defer func() {
		s.Mu.Lock()
		delete(s.Pending, requestID)
		s.Mu.Unlock()
	}()

	select {
	case <-ctx.Done():
		return protocol.ToolResultMessage{}, ctx.Err()
	case result := <-ch:
		return result.Message, result.Err
	}
}

func (m *Manager) ResolveToolResult(sessionID, requestID string, msg protocol.ToolResultMessage, err error) {
	m.mu.RLock()
	s := m.sessions[sessionID]
	m.mu.RUnlock()
	if s == nil {
		return
	}
	s.Mu.Lock()
	ch := s.Pending[requestID]
	s.Mu.Unlock()
	if ch == nil {
		return
	}
	ch <- toolResult{Message: msg, Err: err}
}
