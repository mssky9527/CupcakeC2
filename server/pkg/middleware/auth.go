package middleware

import (
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"cupcake-server/pkg/model"
	"cupcake-server/pkg/store"
)

var (
	cachedToken   string
	lastTokenLoad time.Time
	allowedIPs    string
	lastIPLoad    time.Time
	mcpEnabled    bool
	lastMcpSync   time.Time
)

// GetCurrentToken returns the active API token (refreshes if needed)
func GetCurrentToken() string {
	now := time.Now()
	if cachedToken == "" || now.Sub(lastTokenLoad) > 1*time.Minute {
		cachedToken = store.GetSetting("system_api_token")
		lastTokenLoad = now
		if cachedToken == "" {
			cachedToken = "cupcake_master_key_default_replace_me"
		}
	}
	return cachedToken
}

func AuthMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		path := c.Request.URL.Path
		
		// 1. 静态资源和登录接口免鉴权
		if path == "/api/auth/login" || strings.HasPrefix(path, "/api/s/") || !strings.HasPrefix(path, "/api") {
			c.Next()
			return
		}

		// 2. IP 白名单防御 (由基础物理层面阻断爆破)
		now := time.Now()
		if allowedIPs == "" || now.Sub(lastIPLoad) > 1*time.Minute {
			allowedIPs = store.GetSetting("allowed_ips")
			lastIPLoad = now
		}

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
				// OpSec: Return generic 403 without detail
				c.Status(http.StatusForbidden)
				c.Abort()
				return
			}
		}

		// 3. Token 提取 (支持 Header 和 Query)
		authHeader := c.GetHeader("Authorization")
		token := ""
		if strings.HasPrefix(authHeader, "Bearer ") {
			token = strings.TrimPrefix(authHeader, "Bearer ")
		} else {
			token = c.Query("token")
		}

		if token == "" {
			c.Status(http.StatusUnauthorized)
			c.Abort()
			return
		}

		// 4. Token 校验与同步 (带 1 分钟缓存，减小 DB 压力)
		if cachedToken == "" || now.Sub(lastTokenLoad) > 1*time.Minute {
			cachedToken = store.GetSetting("system_api_token")
			lastTokenLoad = now
			
			// Sync MCP Status too
			mcpStatus := store.GetSetting("system_mcp_enabled")
			mcpEnabled = (mcpStatus == "true" || mcpStatus == "")
			
			if cachedToken == "" {
				cachedToken = "cupcake_master_key_default_replace_me"
			}
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

		// OpSec/Robustness: If query-based token fails, it might be due to URL decoding issues (+ -> space) 
		// or truncation at '&'. This is common for WebSocket handshakes.
		if !isAuthenticated && c.Query("token") != "" {
			// 1. Try restoring '+' from spaces (common Gin/URL decoding behavior)
			if strings.Contains(token, " ") {
				fixedToken := strings.ReplaceAll(token, " ", "+")
				if err := store.DB.Where("token = ?", fixedToken).First(&user).Error; err == nil {
					if user.IsActive {
						isAuthenticated = true
					}
				}
			}
			
			// 2. Deep scan: If it still fails, the token might be truncated at '&'.
			// We scan the RawQuery for the full 'token=' value.
			if !isAuthenticated {
				raw := c.Request.URL.RawQuery
				if strings.Contains(raw, "token=") {
					// Find token= and extract until end of string or next legitimate param
					parts := strings.Split(raw, "token=")
					if len(parts) > 1 {
						fullToken := parts[1]
						// If there were other params after token, they might be part of the token itself 
						// if the client failed to encode. We try the whole remaining part.
						if err := store.DB.Where("token = ?", fullToken).First(&user).Error; err == nil {
							if user.IsActive {
								isAuthenticated = true
							}
						}
					}
				}
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
