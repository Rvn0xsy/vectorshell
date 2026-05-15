package api

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"mime/multipart"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/arch/vectorshell-go/internal/agent"
	"github.com/arch/vectorshell-go/internal/config"
	"github.com/arch/vectorshell-go/internal/tunnel"
	"github.com/arch/vectorshell-go/internal/events"
	"github.com/arch/vectorshell-go/internal/mcp"
	"github.com/arch/vectorshell-go/internal/protocol"
	"github.com/arch/vectorshell-go/internal/session"
	"github.com/arch/vectorshell-go/internal/store"
	"github.com/google/uuid"
	"github.com/gorilla/websocket"
)

type Server struct {
	cfg           config.Config
	mgr           *session.Manager
	agentSvc      *agent.Service
	bus           *events.Bus
	upgrader      websocket.Upgrader
	conversations map[string]string
	store         *store.Store
	mu            sync.RWMutex
}

type toolRequest struct {
	InstallID string         `json:"install_id"`
	ToolName  string         `json:"tool_name"`
	Args      map[string]any `json:"args"`
	TimeoutMS int64          `json:"timeout_ms"`
}

type agentRequest struct {
	InstallID string `json:"install_id"`
	Prompt    string `json:"prompt"`
}

type conversationCreateRequest struct {
	InstallID string `json:"install_id"`
	Title     string `json:"title"`
}

type conversationCreateResponse struct {
	ConversationID string `json:"conversation_id"`
}

type conversationMessageRequest struct {
	Prompt  string `json:"prompt"`
	Message string `json:"message"`
}

func NewServer(cfg config.Config, mgr *session.Manager, agentSvc *agent.Service, db *store.Store) *Server {
	return &Server{
		cfg:           cfg,
		mgr:           mgr,
		agentSvc:      agentSvc,
		bus:           events.NewBus(),
		upgrader:      websocket.Upgrader{CheckOrigin: func(r *http.Request) bool { return true }},
		conversations: make(map[string]string),
		store:         db,
	}
}

func (s *Server) Routes() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/api/health", s.handleHealth)
	mux.HandleFunc("/api/sessions", s.handleSessions)
	mux.HandleFunc("/api/sessions/", s.handleSessionRoutes)
	mux.HandleFunc("/api/conversations", s.handleCreateConversation)
	mux.HandleFunc("/api/conversations/", s.handleConversationRoutes)
	mux.HandleFunc("/api/artifacts", s.handleArtifacts)
	mux.HandleFunc("/api/artifacts/", s.handleArtifactRoutes)
	mux.HandleFunc("/api/clients/generate", s.handleGenerateClient)
	mux.HandleFunc("/api/clients/download", s.handleDownloadClient)
	mux.HandleFunc("/api/tools", s.handleTools)
	mux.HandleFunc("/api/agent", s.handleAgent)
	mux.HandleFunc("/mcp", s.handleMCP)
	mux.HandleFunc(s.cfg.Server.WSPath, s.handleWS)
	s.mountUI(mux)
	return mux
}

func (s *Server) handleSessionRoutes(w http.ResponseWriter, r *http.Request) {
	path := strings.TrimPrefix(r.URL.Path, "/api/sessions/")
	parts := strings.Split(strings.Trim(path, "/"), "/")
	if len(parts) != 2 {
		writeError(w, http.StatusNotFound, "not found")
		return
	}
	installID := parts[0]
	switch parts[1] {
	case "events":
		s.handleSessionEvents(w, r, installID)
	case "history":
		s.handleSessionHistory(w, r, installID)
	case "tools":
		s.handleSessionTools(w, r, installID)
	case "clean":
		s.handleSessionClean(w, r, installID)
	default:
		writeError(w, http.StatusNotFound, "not found")
	}
}

func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{"ok": true})
}

func (s *Server) handleSessions(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"sessions": s.mgr.List()})
}

func (s *Server) handleSessionTools(w http.ResponseWriter, r *http.Request, installID string) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	var req toolRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid json")
		return
	}
	req.InstallID = installID
	s.handleToolRequest(w, r, req)
}

func (s *Server) handleTools(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	var req toolRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid json")
		return
	}
	if req.InstallID == "" || req.ToolName == "" {
		writeError(w, http.StatusBadRequest, "install_id and tool_name are required")
		return
	}
	s.handleToolRequest(w, r, req)
}

func (s *Server) handleToolRequest(w http.ResponseWriter, r *http.Request, req toolRequest) {
	if req.ToolName == "" {
		writeError(w, http.StatusBadRequest, "tool_name is required")
		return
	}
	timeout := 30 * time.Second
	if req.TimeoutMS > 0 {
		timeout = time.Duration(req.TimeoutMS) * time.Millisecond
	}
	ctx, cancel := context.WithTimeout(r.Context(), timeout)
	defer cancel()
	result, err := s.dispatchToolRequest(ctx, req, timeout)
	if err != nil {
		status := http.StatusBadGateway
		if errors.Is(err, session.ErrSessionNotFound) {
			status = http.StatusNotFound
		}
		writeError(w, status, err.Error())
		return
	}
	var data any
	if len(result) > 0 {
		if err := json.Unmarshal(result, &data); err != nil {
			data = string(result)
		}
	}
	writeJSON(w, http.StatusOK, map[string]any{"ok": true, "tool_name": req.ToolName, "data": data})
}

func (s *Server) dispatchToolRequest(ctx context.Context, req toolRequest, timeout time.Duration) (json.RawMessage, error) {
	switch req.ToolName {
	case "upload_file":
		src, err := s.resolveServerPathArg(req.Args["src"])
		if err != nil {
			return nil, err
		}
		dst, err := pathFromArg(req.Args["dst"])
		if err != nil {
			return nil, err
		}
		if src == "" || dst == "" {
			return nil, fmt.Errorf("upload_file requires src and dst")
		}
		return s.mgr.UploadFile(ctx, req.InstallID, src, dst, timeout)
	case "download_file":
		src, err := pathFromArg(req.Args["src"])
		if err != nil {
			return nil, err
		}
		if src == "" {
			return nil, fmt.Errorf("download_file requires src")
		}
		if isArtifactDestination(req.Args["dst"]) {
			path := s.newArtifactPath(filepath.Base(src))
			result, err := s.mgr.DownloadFile(ctx, req.InstallID, src, path, timeout)
			if err != nil {
				return nil, err
			}
			record, err := s.registerArtifactFromPath(path, filepath.Base(src), "application/octet-stream")
			if err != nil {
				return nil, err
			}
			var payload map[string]any
			_ = json.Unmarshal(result, &payload)
			payload["artifact_id"] = record.ID
			payload["size_bytes"] = record.Size
			return json.Marshal(payload)
		}
		dst, err := pathFromArg(req.Args["dst"])
		if err != nil {
			return nil, err
		}
		if dst == "" {
			return nil, fmt.Errorf("download_file requires dst")
		}
		return s.mgr.DownloadFile(ctx, req.InstallID, src, dst, timeout)
	default:
		return s.mgr.DispatchTool(ctx, req.InstallID, req.ToolName, req.Args, timeout)
	}
}

func (s *Server) handleAgent(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	var req agentRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid json")
		return
	}
	if req.InstallID == "" || strings.TrimSpace(req.Prompt) == "" {
		writeError(w, http.StatusBadRequest, "install_id and prompt are required")
		return
	}
	answer, err := s.agentSvc.Run(r.Context(), req.InstallID, req.Prompt, s.cfg.Skill.Dir, s.cfg.Skill.Enabled)
	if err != nil {
		writeError(w, http.StatusBadGateway, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]string{"answer": answer})
}

func (s *Server) handleCreateConversation(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	var req conversationCreateRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid json")
		return
	}
	if strings.TrimSpace(req.InstallID) == "" {
		writeError(w, http.StatusBadRequest, "install_id is required")
		return
	}
	conversationID := uuid.NewString()
	s.mu.Lock()
	s.conversations[conversationID] = req.InstallID
	s.mu.Unlock()
	if err := s.store.SetConversation(req.InstallID, conversationID); err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	s.bus.Publish(conversationKey(conversationID), events.ConversationEvent{
		Event:          "conversation.started",
		ConversationID: conversationID,
		Timestamp:      time.Now().UTC().Format(time.RFC3339),
		InstallID:      req.InstallID,
	})
	s.bus.Publish(sessionKey(req.InstallID), events.ConversationEvent{
		Event:          "conversation.started",
		ConversationID: conversationID,
		Timestamp:      time.Now().UTC().Format(time.RFC3339),
		InstallID:      req.InstallID,
	})
	writeJSON(w, http.StatusOK, conversationCreateResponse{ConversationID: conversationID})
}

func (s *Server) handleConversationRoutes(w http.ResponseWriter, r *http.Request) {
	path := strings.TrimPrefix(r.URL.Path, "/api/conversations/")
	parts := strings.Split(strings.Trim(path, "/"), "/")
	if len(parts) != 2 {
		writeError(w, http.StatusNotFound, "not found")
		return
	}
	conversationID := parts[0]
	switch parts[1] {
	case "messages":
		s.handleConversationMessage(w, r, conversationID)
	case "events":
		s.handleConversationEvents(w, r, conversationID)
	default:
		writeError(w, http.StatusNotFound, "not found")
	}
}

func (s *Server) handleConversationMessage(w http.ResponseWriter, r *http.Request, conversationID string) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	installID := strings.TrimSpace(r.URL.Query().Get("install_id"))
	if installID == "" {
		s.mu.RLock()
		installID = s.conversations[conversationID]
		s.mu.RUnlock()
	}
	if installID == "" {
		storedInstallID, err := s.store.GetInstallIDByConversation(conversationID)
		if err != nil {
			writeError(w, http.StatusInternalServerError, err.Error())
			return
		}
		installID = storedInstallID
	}
	if installID == "" {
		writeError(w, http.StatusBadRequest, "install_id query parameter is required")
		return
	}
	var req conversationMessageRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid json")
		return
	}
	if req.Prompt == "" {
		req.Prompt = req.Message
	}
	if strings.TrimSpace(req.Prompt) == "" {
		writeError(w, http.StatusBadRequest, "prompt is required")
		return
	}
	s.bus.Publish(conversationKey(conversationID), events.ConversationEvent{
		Event:          "agent.message",
		ConversationID: conversationID,
		Timestamp:      time.Now().UTC().Format(time.RFC3339),
		Role:           "user",
		Content:        req.Prompt,
		InstallID:      installID,
	})
	if err := s.store.AppendMessage(installID, conversationID, "user", req.Prompt); err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	go func() {
		fullPrompt := s.buildPromptWithHistory(installID, req.Prompt)
		answer, err := s.agentSvc.RunWithEvents(context.Background(), installID, fullPrompt, s.cfg.Skill.Dir, s.cfg.Skill.Enabled, func(payload agent.EventPayload) {
			s.publishAgentEvent(conversationID, payload)
		})
		if err != nil {
			errorEvent := events.ConversationEvent{
				Event:          "error",
				ConversationID: conversationID,
				Timestamp:      time.Now().UTC().Format(time.RFC3339),
				Code:           "agent_error",
				Message:        err.Error(),
				InstallID:      installID,
			}
			s.bus.Publish(conversationKey(conversationID), errorEvent)
			s.bus.Publish(sessionKey(installID), errorEvent)
			return
		}
		if err := s.store.AppendMessage(installID, conversationID, "assistant", answer); err != nil {
			log.Printf("append assistant history failed: %v", err)
		}
		finishedEvent := events.ConversationEvent{
			Event:          "conversation.finished",
			ConversationID: conversationID,
			Timestamp:      time.Now().UTC().Format(time.RFC3339),
			OK:             true,
			InstallID:      installID,
		}
		s.bus.Publish(conversationKey(conversationID), finishedEvent)
		s.bus.Publish(sessionKey(installID), finishedEvent)
	}()
	writeJSON(w, http.StatusAccepted, map[string]string{"status": "accepted"})
}

func (s *Server) handleConversationEvents(w http.ResponseWriter, r *http.Request, conversationID string) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")
	w.Header().Set("X-Accel-Buffering", "no")
	flusher, ok := w.(http.Flusher)
	if !ok {
		writeError(w, http.StatusInternalServerError, "streaming unsupported")
		return
	}
	stream, unsubscribe := s.bus.Subscribe(conversationKey(conversationID))
	defer unsubscribe()
	ctx := r.Context()
	_, _ = io.WriteString(w, ": connected\n\n")
	flusher.Flush()
	keepalive := time.NewTicker(15 * time.Second)
	defer keepalive.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-keepalive.C:
			_, _ = io.WriteString(w, ": keepalive\n\n")
			flusher.Flush()
		case payload, ok := <-stream:
			if !ok {
				return
			}
			_, _ = io.WriteString(w, "data: ")
			_, _ = w.Write(payload)
			_, _ = io.WriteString(w, "\n\n")
			flusher.Flush()
		}
	}
}

func (s *Server) handleSessionEvents(w http.ResponseWriter, r *http.Request, installID string) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")
	w.Header().Set("X-Accel-Buffering", "no")
	flusher, ok := w.(http.Flusher)
	if !ok {
		writeError(w, http.StatusInternalServerError, "streaming unsupported")
		return
	}
	stream, unsubscribe := s.bus.Subscribe(sessionKey(installID))
	defer unsubscribe()
	ctx := r.Context()
	_, _ = io.WriteString(w, ": connected\n\n")
	flusher.Flush()
	keepalive := time.NewTicker(15 * time.Second)
	defer keepalive.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-keepalive.C:
			_, _ = io.WriteString(w, ": keepalive\n\n")
			flusher.Flush()
		case payload, ok := <-stream:
			if !ok {
				return
			}
			_, _ = io.WriteString(w, "data: ")
			_, _ = w.Write(payload)
			_, _ = io.WriteString(w, "\n\n")
			flusher.Flush()
		}
	}
}

func (s *Server) handleSessionHistory(w http.ResponseWriter, r *http.Request, installID string) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	s.mu.RLock()
	state, err := s.store.GetConversationState(installID)
	s.mu.RUnlock()
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	if state == nil {
		writeJSON(w, http.StatusOK, map[string]any{"install_id": installID, "conversation_id": "", "messages": []store.HistoryMessage{}})
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"install_id": installID, "conversation_id": state.ConversationID, "messages": state.Messages})
}

func (s *Server) handleSessionClean(w http.ResponseWriter, r *http.Request, installID string) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	if err := s.store.ClearInstall(installID); err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	s.mu.Lock()
	for conversationID, mappedInstallID := range s.conversations {
		if mappedInstallID == installID {
			delete(s.conversations, conversationID)
		}
	}
	s.mu.Unlock()
	writeJSON(w, http.StatusOK, map[string]any{"ok": true, "install_id": installID})
}

func (s *Server) buildPromptWithHistory(installID, latest string) string {
	state, err := s.store.GetConversationState(installID)
	if err != nil || state == nil || len(state.Messages) == 0 {
		return latest
	}
	var builder strings.Builder
	builder.WriteString("Conversation so far:\n")
	for _, message := range state.Messages {
		builder.WriteString(message.Role)
		builder.WriteString(": ")
		builder.WriteString(message.Content)
		builder.WriteString("\n")
	}
	builder.WriteString("\nRespond to the latest user message.\n")
	builder.WriteString("latest_user_message: ")
	builder.WriteString(latest)
	return builder.String()
}

func (s *Server) handleArtifacts(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	file, header, err := r.FormFile("file")
	if err != nil {
		writeError(w, http.StatusBadRequest, "file field is required")
		return
	}
	defer file.Close()
	record, err := s.saveArtifactUpload(file, header)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, record)
}

func (s *Server) handleArtifactRoutes(w http.ResponseWriter, r *http.Request) {
	path := strings.TrimPrefix(r.URL.Path, "/api/artifacts/")
	parts := strings.Split(strings.Trim(path, "/"), "/")
	if len(parts) != 2 || parts[1] != "download" {
		writeError(w, http.StatusNotFound, "not found")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	s.mu.RLock()
	record, err := s.store.GetArtifact(parts[0])
	s.mu.RUnlock()
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	if record == nil {
		writeError(w, http.StatusNotFound, "artifact not found")
		return
	}
	w.Header().Set("Content-Disposition", fmt.Sprintf("attachment; filename=%q", record.Name))
	if record.MimeType != "" {
		w.Header().Set("Content-Type", record.MimeType)
	}
	http.ServeFile(w, r, record.Path)
}

func (s *Server) handleGenerateClient(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	var req struct {
		Target string `json:"target"`
	}
	_ = json.NewDecoder(r.Body).Decode(&req)
	if req.Target == "" {
		req.Target = "linux-amd64"
	}
	outputPath, fileName, goos, goarch, err := buildTargetInfo(req.Target)
	if err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	serverURL, err := derivedClientServerURL(s.cfg)
	if err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	moduleRoot := findModuleRoot()
	if err := os.MkdirAll(filepath.Dir(outputPath), 0o755); err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	ldflags := fmt.Sprintf(
		"-X github.com/arch/vectorshell-go/internal/embedded.ServerURL=%s -X github.com/arch/vectorshell-go/internal/embedded.ClientToken=%s -X github.com/arch/vectorshell-go/internal/embedded.ReconnectInterval=%d",
		serverURL, s.cfg.Auth.ClientToken, s.cfg.Client.ReconnectInterval,
	)
	if s.cfg.Tunnel.Enabled {
		ldflags += " -X github.com/arch/vectorshell-go/internal/embedded.TunnelEnabled=true"
		ldflags += fmt.Sprintf(" -X github.com/arch/vectorshell-go/internal/embedded.TunnelPSK=%s", s.cfg.Tunnel.PreSharedKey)
		ldflags += fmt.Sprintf(" -X github.com/arch/vectorshell-go/internal/embedded.TunnelPort=%d", s.cfg.Tunnel.Port)
		if s.cfg.Tunnel.Host != "" {
			ldflags += fmt.Sprintf(" -X github.com/arch/vectorshell-go/internal/embedded.TunnelHost=%s", s.cfg.Tunnel.Host)
		}
		if s.cfg.Tunnel.ProxyHost != "" {
			ldflags += fmt.Sprintf(" -X github.com/arch/vectorshell-go/internal/embedded.TunnelProxyHost=%s", s.cfg.Tunnel.ProxyHost)
		}
		if s.cfg.Tunnel.ProxyPort > 0 {
			ldflags += fmt.Sprintf(" -X github.com/arch/vectorshell-go/internal/embedded.TunnelProxyPort=%d", s.cfg.Tunnel.ProxyPort)
		}
	}
	cmd := exec.Command(
		"go",
		"build",
		"-mod=vendor",
		"-o",
		outputPath,
		"-ldflags",
		ldflags,
		"./cmd/client",
	)
	cmd.Dir = moduleRoot
	cmd.Env = append(os.Environ(), "GOOS="+goos, "GOARCH="+goarch)
	output, err := cmd.CombinedOutput()
	if err != nil {
		writeError(w, http.StatusInternalServerError, fmt.Sprintf("build client failed: %v: %s", err, strings.TrimSpace(string(output))))
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"ok": true, "target": req.Target, "file": fileName})
}

func (s *Server) handleDownloadClient(w http.ResponseWriter, r *http.Request) {
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	target := r.URL.Query().Get("target")
	if target == "" {
		target = "linux-amd64"
	}
	outputPath, fileName, _, _, err := buildTargetInfo(target)
	if err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	if info, err := os.Stat(outputPath); err == nil && !info.IsDir() {
		w.Header().Set("Content-Disposition", fmt.Sprintf("attachment; filename=%q", fileName))
		http.ServeFile(w, r, outputPath)
		return
	}
	writeError(w, http.StatusNotFound, "client binary not found; build cmd/client first")
}

func buildTargetInfo(target string) (outputPath string, fileName string, goos string, goarch string, err error) {
	switch target {
	case "linux-amd64":
		return filepath.Join("build", "clients", "vectorshell-client-linux-amd64"), "vectorshell-client", "linux", "amd64", nil
	case "linux-arm64":
		return filepath.Join("build", "clients", "vectorshell-client-linux-arm64"), "vectorshell-client", "linux", "arm64", nil
	case "windows-amd64":
		return filepath.Join("build", "clients", "vectorshell-client-windows-amd64.exe"), "vectorshell-client.exe", "windows", "amd64", nil
	case "windows-arm64":
		return filepath.Join("build", "clients", "vectorshell-client-windows-arm64.exe"), "vectorshell-client.exe", "windows", "arm64", nil
	case "macos-amd64":
		return filepath.Join("build", "clients", "vectorshell-client-macos-amd64"), "vectorshell-client", "darwin", "amd64", nil
	case "macos-arm64":
		return filepath.Join("build", "clients", "vectorshell-client-macos-arm64"), "vectorshell-client", "darwin", "arm64", nil
	default:
		return "", "", "", "", fmt.Errorf("unsupported target: %s", target)
	}
}

func derivedClientServerURL(cfg config.Config) (string, error) {
	if cfg.Tunnel.Enabled {
		host := cfg.Tunnel.Host
		if host == "" {
			host = "127.0.0.1"
		}
		wsPath := cfg.Server.WSPath
		if wsPath == "" {
			wsPath = "/ws"
		}
		if !strings.HasPrefix(wsPath, "/") {
			wsPath = "/" + wsPath
		}
		return fmt.Sprintf("ws://%s:%d%s", host, cfg.Tunnel.Port, wsPath), nil
	}
	if cfg.Server.PublicURL != "" {
		return cfg.Server.PublicURL, nil
	}
	hostPort := strings.TrimSpace(cfg.Server.Listen)
	if hostPort == "" {
		return "", fmt.Errorf("server.listen is empty")
	}
	if strings.HasPrefix(hostPort, ":") {
		hostPort = "127.0.0.1" + hostPort
	}
	wsPath := cfg.Server.WSPath
	if wsPath == "" {
		wsPath = "/ws"
	}
	if !strings.HasPrefix(wsPath, "/") {
		wsPath = "/" + wsPath
	}
	return "ws://" + hostPort + wsPath, nil
}

func findModuleRoot() string {
	if cwd, err := os.Getwd(); err == nil {
		return cwd
	}
	return "."
}

func (s *Server) handleMCP(w http.ResponseWriter, r *http.Request) {
	if !s.authorized(r) {
		writeError(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	switch r.Method {
	case http.MethodGet:
		w.Header().Set("Content-Type", "text/event-stream")
		w.Header().Set("Cache-Control", "no-cache")
		_, _ = io.WriteString(w, ": mcp keepalive\n\n")
		if flusher, ok := w.(http.Flusher); ok {
			flusher.Flush()
		}
	case http.MethodPost:
		var req mcp.JsonRPCRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			writeJSON(w, http.StatusBadRequest, mcp.Failure(nil, -32700, "parse error"))
			return
		}
		response := s.handleMCPRequest(r.Context(), req)
		writeJSON(w, http.StatusOK, response)
	default:
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
	}
}

func (s *Server) handleMCPRequest(ctx context.Context, req mcp.JsonRPCRequest) mcp.JsonRPCResponse {
	var id any
	if len(req.ID) > 0 {
		_ = json.Unmarshal([]byte(req.ID), &id)
	}
	switch req.Method {
	case "initialize":
		return mcp.Success(id, map[string]any{
			"protocolVersion": "2025-11-25",
			"capabilities":    map[string]any{"tools": map[string]any{}},
			"serverInfo":      map[string]any{"name": "vectorshell", "version": "0.1.0"},
		})
	case "tools/list":
		return mcp.Success(id, map[string]any{"tools": mcp.ToolList()})
	case "tools/call":
		var params mcp.CallToolParams
		if err := json.Unmarshal(req.Params, &params); err != nil {
			return mcp.Failure(id, -32602, "invalid params")
		}
		installID, _ := params.Arguments["install_id"].(string)
		if installID == "" {
			return mcp.Failure(id, -32602, "install_id is required")
		}
		args := make(map[string]any, len(params.Arguments))
		for key, value := range params.Arguments {
			if key == "install_id" {
				continue
			}
			args[key] = value
		}
		result, err := s.dispatchToolRequest(ctx, toolRequest{InstallID: installID, ToolName: params.Name, Args: args}, 60*time.Second)
		if err != nil {
			return mcp.Failure(id, -32002, err.Error())
		}
		return mcp.Success(id, map[string]any{
			"content": []map[string]any{{"type": "text", "text": string(result)}},
		})
	default:
		return mcp.Failure(id, -32601, "method not found")
	}
}

func (s *Server) publishAgentEvent(conversationID string, payload agent.EventPayload) {
	event := events.ConversationEvent{
		Event:          payload.Event,
		ConversationID: conversationID,
		Timestamp:      time.Now().UTC().Format(time.RFC3339),
		RequestID:      payload.RequestID,
		ToolName:       payload.ToolName,
		Args:           payload.Args,
		Role:           payload.Role,
		Content:        payload.Content,
		Final:          payload.Final,
		OK:             payload.OK,
		DurationMS:     payload.DurationMS,
		Data:           payload.Data,
		Message:        payload.Message,
	}
	s.bus.Publish(conversationKey(conversationID), event)
	if payload.InstallID != "" {
		s.bus.Publish(sessionKey(payload.InstallID), event)
	}
}

func conversationKey(conversationID string) string {
	return "conversation:" + conversationID
}

func sessionKey(installID string) string {
	return "session:" + installID
}

func (s *Server) handleWS(w http.ResponseWriter, r *http.Request) {
	conn, err := s.upgrader.Upgrade(w, r, nil)
	if err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	go s.serveConn(conn)
}

func (s *Server) serveConn(conn *websocket.Conn) {
	defer func() {
		if recovered := recover(); recovered != nil {
			log.Printf("websocket session panic: %v", recovered)
		}
	}()
	defer conn.Close()
	var current *session.ClientSession
	for {
		var env protocol.Envelope
		if err := conn.ReadJSON(&env); err != nil {
			log.Printf("websocket read failed: %v", err)
			if current != nil {
				s.mgr.Remove(current.SessionID)
			}
			return
		}
		switch env.Type {
		case "register":
			var msg protocol.RegisterMessage
			if err := json.Unmarshal(env.Payload, &msg); err != nil {
				return
			}
			if msg.Token != s.cfg.Auth.ClientToken {
				_ = conn.WriteJSON(protocol.MustEnvelope("error", uuid.NewString(), map[string]string{"error": "unauthorized"}))
				return
			}
			current = s.mgr.Add(conn, msg)
			ack := protocol.RegisteredMessage{SessionID: current.SessionID, InstallID: current.InstallID}
			if err := conn.WriteJSON(protocol.MustEnvelope("registered", uuid.NewString(), ack)); err != nil {
				return
			}
		case "heartbeat":
			if current != nil {
				s.mgr.Touch(current.SessionID)
			}
		case "tool_result":
			if current == nil {
				continue
			}
			var msg protocol.ToolResultMessage
			if err := json.Unmarshal(env.Payload, &msg); err != nil {
				continue
			}
			s.mgr.ResolveToolResult(current.SessionID, env.ID, msg, nil)
		case "result":
			if current == nil {
				continue
			}
			var msg protocol.ResultMessage
			if err := json.Unmarshal(env.Payload, &msg); err != nil {
				continue
			}
			data, _ := json.Marshal(msg)
			s.mgr.ResolveToolResult(current.SessionID, env.ID, protocol.ToolResultMessage{
				ToolName:   "exec",
				OK:         msg.ExitCode == 0,
				Data:       data,
				Error:      msg.Stderr,
				DurationMS: msg.DurationMS,
			}, nil)
		default:
			log.Printf("ignore message type=%s", env.Type)
		}
	}
}

func (s *Server) authorized(r *http.Request) bool {
	token := strings.TrimSpace(strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer "))
	if token == "" {
		token = strings.TrimSpace(r.URL.Query().Get("token"))
	}
	if s.cfg.Auth.APIToken == "" {
		return true
	}
	return token == s.cfg.Auth.APIToken
}

func (s *Server) mountUI(mux *http.ServeMux) {
	uiPath := "/" + strings.Trim(s.cfg.Server.UIPath, "/")
	if uiPath == "/" {
		return
	}
	if info, err := os.Stat(s.cfg.Server.UIDist); err != nil || !info.IsDir() {
		return
	}
	fileServer := http.FileServer(http.Dir(s.cfg.Server.UIDist))
	mux.Handle(uiPath+"/", http.StripPrefix(uiPath+"/", fileServer))
	mux.Handle("/webapp/", http.StripPrefix("/webapp/", fileServer))
	mux.HandleFunc(uiPath, func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, uiPath+"/", http.StatusMovedPermanently)
	})
	mux.HandleFunc("/webapp", func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, uiPath+"/", http.StatusMovedPermanently)
	})
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/" {
			http.Redirect(w, r, uiPath+"/", http.StatusFound)
			return
		}
		writeError(w, http.StatusNotFound, "not found")
	})
}

func (s *Server) saveArtifactUpload(file multipart.File, header *multipart.FileHeader) (store.ArtifactRecord, error) {
	name := filepath.Base(header.Filename)
	if name == "." || name == string(filepath.Separator) || name == "" {
		name = "artifact.bin"
	}
	path := s.newArtifactPath(name)
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return store.ArtifactRecord{}, err
	}
	out, err := os.Create(path)
	if err != nil {
		return store.ArtifactRecord{}, err
	}
	defer out.Close()
	size, err := io.Copy(out, file)
	if err != nil {
		return store.ArtifactRecord{}, err
	}
	return s.registerArtifact(path, name, size, header.Header.Get("Content-Type")), nil
}

func (s *Server) registerArtifactFromPath(path, name, mimeType string) (store.ArtifactRecord, error) {
	info, err := os.Stat(path)
	if err != nil {
		return store.ArtifactRecord{}, err
	}
	return s.registerArtifact(path, name, info.Size(), mimeType), nil
}

func (s *Server) registerArtifact(path, name string, size int64, mimeType string) store.ArtifactRecord {
	record := store.ArtifactRecord{ID: randomID(), Name: name, Path: path, Size: size, MimeType: mimeType, Created: time.Now().UTC().Format(time.RFC3339)}
	if err := s.store.SaveArtifact(record); err != nil {
		log.Printf("save artifact metadata failed: %v", err)
	}
	return record
}

func (s *Server) newArtifactPath(name string) string {
	return filepath.Join("data", "artifacts", randomID()+"-"+filepath.Base(name))
}

func (s *Server) resolveServerPathArg(value any) (string, error) {
	if path, ok := value.(string); ok {
		return path, nil
	}
	m, ok := value.(map[string]any)
	if !ok {
		return "", fmt.Errorf("path argument must be a string or object")
	}
	if scope, _ := m["scope"].(string); scope == "artifact" {
		artifactID, _ := m["artifact_id"].(string)
		if artifactID == "" {
			return "", fmt.Errorf("artifact_id is required")
		}
		record, err := s.store.GetArtifact(artifactID)
		if err != nil {
			return "", err
		}
		if record == nil {
			return "", fmt.Errorf("artifact not found: %s", artifactID)
		}
		return record.Path, nil
	}
	return pathFromArg(value)
}

func pathFromArg(value any) (string, error) {
	if value == nil {
		return "", nil
	}
	if path, ok := value.(string); ok {
		return path, nil
	}
	m, ok := value.(map[string]any)
	if !ok {
		return "", fmt.Errorf("path argument must be a string or object")
	}
	path, _ := m["path"].(string)
	return path, nil
}

func isArtifactDestination(value any) bool {
	m, ok := value.(map[string]any)
	if !ok {
		return false
	}
	scope, _ := m["scope"].(string)
	return scope == "artifact"
}

func randomID() string {
	buf := make([]byte, 16)
	if _, err := rand.Read(buf); err != nil {
		return uuid.NewString()
	}
	return hex.EncodeToString(buf)
}

func writeJSON(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}

func writeError(w http.ResponseWriter, status int, message string) {
	writeJSON(w, status, map[string]string{"error": message})
}

func Run(cfg config.Config, mgr *session.Manager, agentSvc *agent.Service, db *store.Store) error {
	srv := NewServer(cfg, mgr, agentSvc, db)

	if cfg.Tunnel.Enabled && cfg.Tunnel.Port > 0 {
		tcpLn, err := net.Listen("tcp", fmt.Sprintf(":%d", cfg.Tunnel.Port))
		if err != nil {
			return fmt.Errorf("tunnel listener on :%d: %w", cfg.Tunnel.Port, err)
		}
		tunnelLn := tunnel.NewListener(tcpLn, []byte(cfg.Tunnel.PreSharedKey))
		log.Printf("vectorshell tunnel listener on :%d", cfg.Tunnel.Port)
		go func() {
			if err := http.Serve(tunnelLn, srv.Routes()); err != nil {
				log.Printf("tunnel listener stopped: %v", err)
			}
		}()
	}

	log.Printf("vectorshell server listening on %s", cfg.Server.Listen)
	return http.ListenAndServe(cfg.Server.Listen, srv.Routes())
}
