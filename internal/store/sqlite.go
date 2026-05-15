package store

import (
	"database/sql"
	"fmt"
	"os"
	"path/filepath"
	"time"

	_ "modernc.org/sqlite"
)

type HistoryMessage struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

type ConversationState struct {
	ConversationID string           `json:"conversation_id"`
	InstallID      string           `json:"install_id"`
	Messages       []HistoryMessage `json:"messages"`
}

type ArtifactRecord struct {
	ID       string `json:"artifact_id"`
	Name     string `json:"name"`
	Path     string `json:"path"`
	Size     int64  `json:"size_bytes"`
	MimeType string `json:"mime_type,omitempty"`
	Created  string `json:"created_at"`
}

type Store struct {
	db *sql.DB
}

func Open(path string) (*Store, error) {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return nil, err
	}
	db, err := sql.Open("sqlite", path)
	if err != nil {
		return nil, err
	}
	store := &Store{db: db}
	if err := store.migrate(); err != nil {
		_ = db.Close()
		return nil, err
	}
	return store, nil
}

func (s *Store) Close() error {
	if s == nil || s.db == nil {
		return nil
	}
	return s.db.Close()
}

func (s *Store) migrate() error {
	queries := []string{
		`CREATE TABLE IF NOT EXISTS conversations (
			install_id TEXT PRIMARY KEY,
			conversation_id TEXT NOT NULL,
			updated_at TEXT NOT NULL
		);`,
		`CREATE TABLE IF NOT EXISTS conversation_messages (
			id INTEGER PRIMARY KEY AUTOINCREMENT,
			install_id TEXT NOT NULL,
			conversation_id TEXT NOT NULL,
			role TEXT NOT NULL,
			content TEXT NOT NULL,
			created_at TEXT NOT NULL
		);`,
		`CREATE INDEX IF NOT EXISTS idx_conversation_messages_install_id_id
		 ON conversation_messages(install_id, id);`,
		`CREATE TABLE IF NOT EXISTS artifacts (
			artifact_id TEXT PRIMARY KEY,
			name TEXT NOT NULL,
			path TEXT NOT NULL,
			size_bytes INTEGER NOT NULL,
			mime_type TEXT NOT NULL,
			created_at TEXT NOT NULL
		);`,
	}
	for _, query := range queries {
		if _, err := s.db.Exec(query); err != nil {
			return err
		}
	}
	return nil
}

func (s *Store) SetConversation(installID, conversationID string) error {
	_, err := s.db.Exec(
		`INSERT INTO conversations(install_id, conversation_id, updated_at)
		 VALUES(?, ?, ?)
		 ON CONFLICT(install_id) DO UPDATE SET conversation_id=excluded.conversation_id, updated_at=excluded.updated_at`,
		installID,
		conversationID,
		time.Now().UTC().Format(time.RFC3339),
	)
	return err
}

func (s *Store) AppendMessage(installID, conversationID, role, content string) error {
	_, err := s.db.Exec(
		`INSERT INTO conversation_messages(install_id, conversation_id, role, content, created_at)
		 VALUES(?, ?, ?, ?, ?)`,
		installID,
		conversationID,
		role,
		content,
		time.Now().UTC().Format(time.RFC3339),
	)
	if err != nil {
		return err
	}
	return s.SetConversation(installID, conversationID)
}

func (s *Store) GetConversationState(installID string) (*ConversationState, error) {
	row := s.db.QueryRow(`SELECT conversation_id FROM conversations WHERE install_id = ?`, installID)
	var conversationID string
	if err := row.Scan(&conversationID); err != nil {
		if err == sql.ErrNoRows {
			return nil, nil
		}
		return nil, err
	}
	rows, err := s.db.Query(
		`SELECT role, content FROM conversation_messages WHERE install_id = ? AND conversation_id = ? ORDER BY id ASC`,
		installID,
		conversationID,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	state := &ConversationState{ConversationID: conversationID, InstallID: installID, Messages: []HistoryMessage{}}
	for rows.Next() {
		var message HistoryMessage
		if err := rows.Scan(&message.Role, &message.Content); err != nil {
			return nil, err
		}
		state.Messages = append(state.Messages, message)
	}
	return state, rows.Err()
}

func (s *Store) GetInstallIDByConversation(conversationID string) (string, error) {
	row := s.db.QueryRow(`SELECT install_id FROM conversations WHERE conversation_id = ?`, conversationID)
	var installID string
	if err := row.Scan(&installID); err != nil {
		if err == sql.ErrNoRows {
			return "", nil
		}
		return "", err
	}
	return installID, nil
}

func (s *Store) ClearInstall(installID string) error {
	if _, err := s.db.Exec(`DELETE FROM conversation_messages WHERE install_id = ?`, installID); err != nil {
		return err
	}
	if _, err := s.db.Exec(`DELETE FROM conversations WHERE install_id = ?`, installID); err != nil {
		return err
	}
	return nil
}

func (s *Store) SaveArtifact(record ArtifactRecord) error {
	_, err := s.db.Exec(
		`INSERT INTO artifacts(artifact_id, name, path, size_bytes, mime_type, created_at)
		 VALUES(?, ?, ?, ?, ?, ?)
		 ON CONFLICT(artifact_id) DO UPDATE SET name=excluded.name, path=excluded.path, size_bytes=excluded.size_bytes, mime_type=excluded.mime_type, created_at=excluded.created_at`,
		record.ID,
		record.Name,
		record.Path,
		record.Size,
		record.MimeType,
		record.Created,
	)
	return err
}

func (s *Store) GetArtifact(artifactID string) (*ArtifactRecord, error) {
	row := s.db.QueryRow(`SELECT artifact_id, name, path, size_bytes, mime_type, created_at FROM artifacts WHERE artifact_id = ?`, artifactID)
	var record ArtifactRecord
	if err := row.Scan(&record.ID, &record.Name, &record.Path, &record.Size, &record.MimeType, &record.Created); err != nil {
		if err == sql.ErrNoRows {
			return nil, nil
		}
		return nil, err
	}
	return &record, nil
}

func DefaultDBPath(baseDir string) string {
	return filepath.Join(baseDir, "data", "vectorshell-go.db")
}

func (s *Store) Health() error {
	if s == nil || s.db == nil {
		return fmt.Errorf("store not initialized")
	}
	return s.db.Ping()
}
