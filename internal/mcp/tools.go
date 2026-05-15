package mcp

func ToolList() []Tool {
	return []Tool{
		{
			Name:        "exec",
			Description: "Execute a shell command on the selected client.",
			InputSchema: schema(map[string]any{"command": requiredString("Shell command"), "install_id": requiredString("Target install_id")}, "install_id", "command"),
		},
		{
			Name:        "read_file",
			Description: "Read a file from the selected client.",
			InputSchema: schema(map[string]any{"path": requiredString("Client-side path"), "install_id": requiredString("Target install_id")}, "install_id", "path"),
		},
		{
			Name:        "write_file",
			Description: "Write a file on the selected client.",
			InputSchema: schema(map[string]any{"path": requiredString("Client-side path"), "content": requiredString("Text content"), "install_id": requiredString("Target install_id")}, "install_id", "path", "content"),
		},
		{
			Name:        "upload_file",
			Description: "Upload a server-local file to the selected client.",
			InputSchema: schema(map[string]any{"src": requiredString("Server-local source path"), "dst": requiredString("Client-side destination path"), "install_id": requiredString("Target install_id")}, "install_id", "src", "dst"),
		},
		{
			Name:        "download_file",
			Description: "Download a client file to the server.",
			InputSchema: schema(map[string]any{"src": requiredString("Client-side source path"), "dst": requiredString("Server-local destination path"), "install_id": requiredString("Target install_id")}, "install_id", "src", "dst"),
		},
	}
}

func requiredString(description string) map[string]any {
	return map[string]any{"type": "string", "description": description}
}

func schema(properties map[string]any, required ...string) map[string]any {
	return map[string]any{
		"type":       "object",
		"properties": properties,
		"required":   required,
	}
}
