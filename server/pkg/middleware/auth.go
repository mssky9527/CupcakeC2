package middleware

import (
	"log"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/gin-gonic/gin"
	"cupcake-server/pkg/model"
	"cupcake-server/pkg/store"
)

// tokenCache holds auth-related settings with RWMutex protection (request-path safe).
var tokenCache struct {
	mu            sync.RWMutex
	cachedToken   string
	lastTokenLoad time.Time
	allowedIPs    string
	lastIPLoad    time.Time
	mcpEnabled    bool
	lastMcpSync   time.Time
}

// GetCurrentToken returns the active API token (refreshes if needed).
// Never calls log.Fatal — empty token returns empty string for callers to handle.
func GetCurrentToken() string {
	now := time.Now()
	tokenCache.mu.RLock()
	if tokenCache.cachedToken != "" && now.Sub(tokenCache.lastTokenLoad) <= 1*time.Minute {
		t := tokenCache.cachedToken
		tokenCache.mu.RUnlock()
		return t
	}
	tokenCache.mu.RUnlock()

	tokenCache.mu.Lock()
	defer tokenCache.mu.Unlock()
	// Double-check after acquiring write lock
	if tokenCache.cachedToken != "" && now.Sub(tokenCache.lastTokenLoad) <= 1*time.Minute {
		return tokenCache.cachedToken
	}
	tokenCache.cachedToken = store.GetSetting("system_api_token")
	tokenCache.lastTokenLoad = now
	return tokenCache.cachedToken
}

// allowQueryToken restricts URL token to WS endpoints used by the browser.
func allowQueryToken(path string) bool {
	if strings.HasPrefix(path, "/api/build/logs/") {
		return true
	}
	if strings.HasPrefix(path, "/api/pty/") {
		return true
	}
	if strings.HasPrefix(path, "/api/shell/") {
		return true
	}
	return false
}

func AuthMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		path := c.Request.URL.Path

		// 1. 静态资源和登录接口免鉴权
		if path == "/api/auth/login" || strings.HasPrefix(path, "/api/s/") || !strings.HasPrefix(path, "/api") {
			c.Next()
			return
		}

		// 2. IP 白名单防御
		now := time.Now()
		tokenCache.mu.RLock()
		needIP := tokenCache.allowedIPs == "" || now.Sub(tokenCache.lastIPLoad) > 1*time.Minute
		tokenCache.mu.RUnlock()
		if needIP {
			tokenCache.mu.Lock()
			if tokenCache.allowedIPs == "" || now.Sub(tokenCache.lastIPLoad) > 1*time.Minute {
				tokenCache.allowedIPs = store.GetSetting("allowed_ips")
				tokenCache.lastIPLoad = now
			}
			tokenCache.mu.Unlock()
		}

		tokenCache.mu.RLock()
		allowedIPs := tokenCache.allowedIPs
		tokenCache.mu.RUnlock()

		clientIP := c.ClientIP()
		if allowedIPs != "" {
			ips := strings.Split(allowedIPs, ",")
			isAllowed := false
			for _, ip := range ips {
				if strings.TrimSpace(ip) == clientIP {
					isAllowed = true
					break
				}
			}
			if !isAllowed {
				log.Printf("[Security] Access Denied for IP: %s (Not in whitelist)", clientIP)
				c.Status(http.StatusForbidden)
				c.Abort()
				return
			}
		}

		// 3. Token 提取：优先 Authorization Bearer。
		// Query ?token= only for browser WebSocket upgrades (cannot set headers):
		// /api/build/logs/*, /api/pty/*, /api/shell/* — avoid logging tokens on REST.
		authHeader := c.GetHeader("Authorization")
		token := ""
		if strings.HasPrefix(authHeader, "Bearer ") {
			token = strings.TrimPrefix(authHeader, "Bearer ")
		}
		if token == "" && allowQueryToken(path) {
			token = strings.TrimSpace(c.Query("token"))
		}

		if token == "" {
			c.Status(http.StatusUnauthorized)
			c.Abort()
			return
		}

		// 4. Token 校验与同步 (带 1 分钟缓存，减小 DB 压力) — mutex protected, no log.Fatal
		tokenCache.mu.RLock()
		needToken := tokenCache.cachedToken == "" || now.Sub(tokenCache.lastTokenLoad) > 1*time.Minute
		tokenCache.mu.RUnlock()
		if needToken {
			tokenCache.mu.Lock()
			if tokenCache.cachedToken == "" || now.Sub(tokenCache.lastTokenLoad) > 1*time.Minute {
				tokenCache.cachedToken = store.GetSetting("system_api_token")
				tokenCache.lastTokenLoad = now
				mcpStatus := store.GetSetting("system_mcp_enabled")
				tokenCache.mcpEnabled = (mcpStatus == "true" || mcpStatus == "")
				tokenCache.lastMcpSync = now
			}
			tokenCache.mu.Unlock()
		}

		tokenCache.mu.RLock()
		cachedToken := tokenCache.cachedToken
		mcpEnabled := tokenCache.mcpEnabled
		tokenCache.mu.RUnlock()

		if cachedToken == "" {
			// Request path must not kill the process — misconfiguration → 500
			log.Printf("[Security] No API token configured (system_api_token empty)")
			c.JSON(http.StatusInternalServerError, gin.H{"error": "server auth not configured"})
			c.Abort()
			return
		}

		// 5. 验证 Token (Master Key 或 User Session)
		isAuthenticated := false

		// Check Master Key (MCP)
		if token == cachedToken {
			if !mcpEnabled {
				log.Printf("[Security] MCP API Attempt while service is DISABLED from IP: %s", clientIP)
				c.Status(http.StatusForbidden)
				c.Abort()
				return
			}
			isAuthenticated = true
		} else {
			// Check User Session Token (Web UI)
			var user model.User
			if err := store.DB.Where("token = ?", token).First(&user).Error; err == nil {
				if user.IsActive {
					isAuthenticated = true
				}
			}
		}

		if !isAuthenticated {
			log.Printf("[Security] Invalid token attempt from IP: %s", clientIP)
			c.Status(http.StatusUnauthorized)
			c.Abort()
			return
		}

		c.Next()
	}
}
