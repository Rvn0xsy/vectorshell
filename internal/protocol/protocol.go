package protocol

import "encoding/json"

type Envelope struct {
	Type    string          `json:"type"`
	ID      string          `json:"id"`
	Payload json.RawMessage `json:"payload"`
}

type RegisterMessage struct {
	Token        string   `json:"token"`
	Hostname     string   `json:"hostname"`
	OS           string   `json:"os"`
	Arch         string   `json:"arch"`
	InstallID    string   `json:"install_id"`
	Username     string   `json:"username,omitempty"`
	PID          int      `json:"pid,omitempty"`
	IP           string   `json:"ip,omitempty"`
	BuildUUID    string   `json:"build_uuid,omitempty"`
	Timestamp    int64    `json:"timestamp"`
	Capabilities []string `json:"capabilities,omitempty"`
}

type RegisteredMessage struct {
	SessionID string `json:"session_id"`
	InstallID string `json:"install_id"`
}

type HeartbeatMessage struct {
	Timestamp int64 `json:"timestamp"`
}

type ExecMessage struct {
	Command string `json:"command"`
}

type ResultMessage struct {
	Command    string            `json:"command"`
	Stdout     string            `json:"stdout"`
	Stderr     string            `json:"stderr"`
	ExitCode   int               `json:"exit_code"`
	DurationMS int64             `json:"duration_ms"`
	CWD        string            `json:"cwd"`
	Env        map[string]string `json:"env,omitempty"`
}

type ToolCallMessage struct {
	ToolName  string          `json:"tool_name"`
	Args      json.RawMessage `json:"args"`
	TimeoutMS int64           `json:"timeout_ms,omitempty"`
}

type ToolResultMessage struct {
	ToolName   string          `json:"tool_name"`
	OK         bool            `json:"ok"`
	Data       json.RawMessage `json:"data,omitempty"`
	Error      string          `json:"error,omitempty"`
	DurationMS int64           `json:"duration_ms"`
}

func MustEnvelope(kind, id string, payload any) Envelope {
	data, err := json.Marshal(payload)
	if err != nil {
		panic(err)
	}
	return Envelope{Type: kind, ID: id, Payload: data}
}
