package skills

import (
	"context"
	"os"
	"path/filepath"

	localbk "github.com/cloudwego/eino-ext/adk/backend/local"
	"github.com/cloudwego/eino/adk"
	"github.com/cloudwego/eino/adk/middlewares/skill"
)

func MiddlewareFromDir(ctx context.Context, dir string) (adk.ChatModelAgentMiddleware, error) {
	if dir == "" {
		return nil, nil
	}
	absDir, err := filepath.Abs(dir)
	if err != nil {
		return nil, err
	}
	stat, err := os.Stat(absDir)
	if err != nil || !stat.IsDir() {
		return nil, nil
	}
	backend, err := localbk.NewBackend(ctx, &localbk.Config{})
	if err != nil {
		return nil, err
	}
	skillBackend, err := skill.NewBackendFromFilesystem(ctx, &skill.BackendFromFilesystemConfig{
		Backend: backend,
		BaseDir: absDir,
	})
	if err != nil {
		return nil, err
	}
	return skill.NewMiddleware(ctx, &skill.Config{Backend: skillBackend})
}
