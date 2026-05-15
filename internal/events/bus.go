package events

import (
	"encoding/json"
	"sync"
	"time"
)

type Bus struct {
	mu       sync.RWMutex
	channels map[string][]chan []byte
}

func NewBus() *Bus {
	return &Bus{channels: make(map[string][]chan []byte)}
}

func (b *Bus) Subscribe(key string) (<-chan []byte, func()) {
	ch := make(chan []byte, 32)
	b.mu.Lock()
	b.channels[key] = append(b.channels[key], ch)
	b.mu.Unlock()
	return ch, func() {
		b.mu.Lock()
		defer b.mu.Unlock()
		subs := b.channels[key]
		for idx, candidate := range subs {
			if candidate == ch {
				b.channels[key] = append(subs[:idx], subs[idx+1:]...)
				close(candidate)
				break
			}
		}
		if len(b.channels[key]) == 0 {
			delete(b.channels, key)
		}
	}
}

func (b *Bus) Publish(key string, payload any) {
	data, err := json.Marshal(payload)
	if err != nil {
		return
	}
	b.mu.RLock()
	defer b.mu.RUnlock()
	for _, ch := range b.channels[key] {
		select {
		case ch <- data:
		default:
		}
	}
}

type ConversationEvent struct {
	Event          string         `json:"event"`
	ConversationID string         `json:"conversation_id"`
	Timestamp      string         `json:"timestamp"`
	RequestID      string         `json:"request_id,omitempty"`
	ToolName       string         `json:"tool_name,omitempty"`
	Args           map[string]any `json:"args,omitempty"`
	Role           string         `json:"role,omitempty"`
	Content        string         `json:"content,omitempty"`
	Final          bool           `json:"final,omitempty"`
	OK             bool           `json:"ok,omitempty"`
	DurationMS     int64          `json:"duration_ms,omitempty"`
	Data           any            `json:"data,omitempty"`
	Code           string         `json:"code,omitempty"`
	Message        string         `json:"message,omitempty"`
	InstallID      string         `json:"install_id,omitempty"`
}

func NewConversationEvent(event, conversationID string) ConversationEvent {
	return ConversationEvent{
		Event:          event,
		ConversationID: conversationID,
		Timestamp:      time.Now().UTC().Format(time.RFC3339),
	}
}
