package controllers

import (
	"encoding/json"
	"io"
	"log"
	"net/http"
	"strconv"
	"time"

	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/utils"
	"cupcake-server/services"

	"github.com/gin-gonic/gin"
	"github.com/gorilla/websocket"
)

// DesktopStatus GET /api/desktop/:uuid/status
// Reports yamux availability + busy; does not auto-stage module.
func DesktopStatus(c *gin.Context) {
	uuidStr := c.Param("uuid")
	val, ok := globals.Clients.Load(uuidStr)
	if !ok {
		c.JSON(http.StatusNotFound, gin.H{"error": "agent offline", "desktop_ready": false})
		return
	}
	client := val.(*globals.Client)
	hasYamux := client.YamuxSession != nil && !client.YamuxSession.IsClosed()
	transport := client.Transport
	if transport == "" {
		if client.TCPConn != nil || hasYamux {
			transport = "tcp"
		} else {
			transport = "websocket"
		}
	}
	// Yamux present is the real gate (TCP path); do not require string == "tcp" only.
	ready := hasYamux
	c.JSON(http.StatusOK, gin.H{
		"uuid":           uuidStr,
		"transport":      transport,
		"yamux":          hasYamux,
		"desktop_busy":   services.HasDesktopSession(uuidStr),
		"desktop_ready":  ready,
		"module_hint":    "请先在「模块」面板加载 L2 模块 desktop，再点连接。Agent 为 standard/TCP 即可（无需 feature=desktop）。",
		"ws_unsupported": !hasYamux,
	})
}

func desktopNotify(ws *websocket.Conn, code, msg string) {
	_ = ws.WriteMessage(websocket.TextMessage, mustJSON(map[string]string{
		"code": code,
		"msg":  msg,
	}))
	log.Printf("[desktop] notify code=%s msg=%s", code, msg)
}

// StreamDesktop GET /api/desktop/:uuid  (admin WebSocket binary CPXD relay)
func StreamDesktop(c *gin.Context) {
	uuidStr := c.Param("uuid")
	log.Printf("[desktop] StreamDesktop open request agent=%s remote=%s", uuidStr, c.ClientIP())

	val, ok := globals.Clients.Load(uuidStr)
	if !ok {
		log.Printf("[desktop] agent offline %s", uuidStr)
		c.JSON(http.StatusNotFound, gin.H{"error": "agent offline", "code": "offline"})
		return
	}
	client := val.(*globals.Client)
	if client.YamuxSession == nil || client.YamuxSession.IsClosed() {
		log.Printf("[desktop] no yamux for %s transport=%s", uuidStr, client.Transport)
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "bridge_unavailable",
			"code":  "bridge_unavailable",
			"msg":   "Desktop requires TCP Yamux agent; WebSocket-only agents are not supported",
		})
		return
	}

	ws, err := globals.AdminUpgrader.Upgrade(c.Writer, c.Request, nil)
	if err != nil {
		log.Printf("[desktop] upgrade failed agent=%s: %v", uuidStr, err)
		return
	}
	defer ws.Close()

	stream, err := services.OpenDesktopToAgent(uuidStr)
	if err != nil {
		code := "error"
		if err == services.DesktopBusyError {
			code = "busy"
		}
		log.Printf("[desktop] OpenDesktopToAgent failed agent=%s: %v", uuidStr, err)
		desktopNotify(ws, code, err.Error())
		return
	}
	// Always release on exit (idempotent)
	defer services.ReleaseDesktop(uuidStr)

	fps, _ := strconv.Atoi(c.DefaultQuery("fps", "5"))
	quality, _ := strconv.Atoi(c.DefaultQuery("quality", "75"))
	maxW, _ := strconv.Atoi(c.DefaultQuery("max_w", "1280"))
	if err := services.WriteDesktopHello(stream, fps, quality, maxW, "jpeg", utils.DesktopDefaultMaxBps); err != nil {
		log.Printf("[desktop] hello write failed %s: %v", uuidStr, err)
		desktopNotify(ws, "hello_failed", "failed to send HELLO to agent: "+err.Error())
		return
	}
	log.Printf("[desktop] HELLO sent agent=%s fps=%d max_w=%d", uuidStr, fps, maxW)

	// Wait for first agent message (HELLO_ACK or ERROR) with timeout — never hang silent.
	_ = stream.SetReadDeadline(time.Now().Add(8 * time.Second))
	hdr := make([]byte, utils.DesktopHeaderLen)
	if _, err := io.ReadFull(stream, hdr); err != nil {
		log.Printf("[desktop] no response from agent %s: %v (agent missing feature=desktop?)", uuidStr, err)
		desktopNotify(ws, "agent_timeout",
			"agent did not reply to HELLO (rebuild agent with feature desktop, or stream closed)")
		return
	}
	_ = stream.SetReadDeadline(time.Time{}) // clear

	env, err := utils.ParseDesktopHeader(hdr)
	if err != nil {
		log.Printf("[desktop] bad header from agent %s: %v", uuidStr, err)
		desktopNotify(ws, "protocol_error", "invalid CPXD header from agent")
		return
	}
	payload := make([]byte, env.PayloadLen)
	if env.PayloadLen > 0 {
		if _, err := io.ReadFull(stream, payload); err != nil {
			desktopNotify(ws, "protocol_error", "truncated first agent payload")
			return
		}
	}
	first := append(append([]byte{}, hdr...), payload...)

	if env.MsgType == utils.DesktopMsgError {
		// Surface agent ERROR JSON to UI
		var ej map[string]string
		code, msg := "agent_error", string(payload)
		if json.Unmarshal(payload, &ej) == nil {
			if ej["code"] != "" {
				code = ej["code"]
			}
			if ej["msg"] != "" {
				msg = ej["msg"]
			}
		}
		log.Printf("[desktop] agent ERROR %s: %s", code, msg)
		desktopNotify(ws, code, msg)
		return
	}

	// Forward HELLO_ACK (or first frame) to browser
	if err := ws.WriteMessage(websocket.BinaryMessage, first); err != nil {
		return
	}
	log.Printf("[desktop] first msg type=0x%02x len=%d to admin", env.MsgType, len(first))

	// Agent → Admin
	done := make(chan struct{})
	go func() {
		defer close(done)
		bufHdr := make([]byte, utils.DesktopHeaderLen)
		for {
			if _, err := io.ReadFull(stream, bufHdr); err != nil {
				log.Printf("[desktop] agent read end %s: %v", uuidStr, err)
				return
			}
			e2, err := utils.ParseDesktopHeader(bufHdr)
			if err != nil {
				log.Printf("[desktop] agent bad frame %s: %v", uuidStr, err)
				return
			}
			pl := make([]byte, e2.PayloadLen)
			if e2.PayloadLen > 0 {
				if _, err := io.ReadFull(stream, pl); err != nil {
					return
				}
			}
			frameLen := utils.DesktopHeaderLen + int(e2.PayloadLen)
			if e2.MsgType == utils.DesktopMsgFrame && !services.DesktopAllowFrame(uuidStr, frameLen) {
				continue
			}
			// Agent ERROR mid-session → surface as text too
			if e2.MsgType == utils.DesktopMsgError {
				desktopNotify(ws, "agent_error", string(pl))
				return
			}
			msg := append(append([]byte{}, bufHdr...), pl...)
			if err := ws.WriteMessage(websocket.BinaryMessage, msg); err != nil {
				return
			}
		}
	}()

	// Admin → Agent (INPUT / STOP / CONFIG)
	for {
		select {
		case <-done:
			_ = services.WriteDesktopStop(stream)
			desktopNotify(ws, "stream_closed", "agent stream ended")
			return
		default:
		}
		mt, data, err := ws.ReadMessage()
		if err != nil {
			_ = services.WriteDesktopStop(stream)
			log.Printf("[desktop] admin ws closed agent=%s: %v", uuidStr, err)
			return
		}
		if mt == websocket.TextMessage {
			var cmd map[string]string
			if json.Unmarshal(data, &cmd) == nil && cmd["type"] == "stop" {
				_ = services.WriteDesktopStop(stream)
				return
			}
			continue
		}
		if mt == websocket.BinaryMessage && len(data) >= utils.DesktopHeaderLen {
			if _, err := stream.Write(data); err != nil {
				desktopNotify(ws, "write_failed", err.Error())
				return
			}
		}
	}
}

func mustJSON(v interface{}) []byte {
	b, _ := json.Marshal(v)
	return b
}
