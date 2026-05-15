package mcp

import "encoding/json"

type JsonRPCRequest struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      json.RawMessage `json:"id,omitempty"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params,omitempty"`
}

type JsonRPCResponse struct {
	JSONRPC string     `json:"jsonrpc"`
	ID      any        `json:"id,omitempty"`
	Result  any        `json:"result,omitempty"`
	Error   *ErrorBody `json:"error,omitempty"`
}

type ErrorBody struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

type Tool struct {
	Name        string         `json:"name"`
	Description string         `json:"description,omitempty"`
	InputSchema map[string]any `json:"inputSchema"`
}

type CallToolParams struct {
	Name      string         `json:"name"`
	Arguments map[string]any `json:"arguments"`
}

func Success(id any, result any) JsonRPCResponse {
	return JsonRPCResponse{JSONRPC: "2.0", ID: id, Result: result}
}

func Failure(id any, code int, message string) JsonRPCResponse {
	return JsonRPCResponse{JSONRPC: "2.0", ID: id, Error: &ErrorBody{Code: code, Message: message}}
}
