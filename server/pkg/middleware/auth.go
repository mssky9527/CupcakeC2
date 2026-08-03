package middleware

import (
	"crypto/subtle"
	"fmt"
	"log"
	"net/netip"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/gin-gonic/gin"

	"cupcake-server/pkg/model"
	"cupcake-server/pkg/store"
)

const (
	principalContextKey = "auth.principal"
	mcpTokenSetting    = "mcp_api_token"
	mcpEnabledSetting  = "system_mcp_enabled"
	mcpCIDRSetting     = "mcp_allowed_cidrs"
	mcpReadOnlySetting = "mcp_read_only"
)

// Principal identifies the authenticated caller. MCP is deliberately not an
// administrator principal: it has a separately constrained capability policy.
type Principal struct {
	Kind     string
	Username string
	Role     string
}

type mcpPolicy struct {
	Token    string
	Enabled  bool
	CIDRs    string
	ReadOnly bool
}

var tokenCache struct {
	mu          sync.RWMutex
	policy      mcpPolicy
	lastLoaded  time.Time
	allowedIPs  string
	lastIPLoad  time.Time
}

// InvalidateAuthCache applies configuration and token rotations immediately.
func InvalidateAuthCache() {
	tokenCache.mu.Lock()
	tokenCache.lastLoaded = time.Time{}
	tokenCache.lastIPLoad = time.Time{}
	tokenCache.policy = mcpPolicy{}
	tokenCache.allowedIPs = ""
	tokenCache.mu.Unlock()
}

func loadMCPPolicy(now time.Time) mcpPolicy {
	tokenCache.mu.RLock()
	if !tokenCache.lastLoaded.IsZero() && now.Sub(tokenCache.lastLoaded) <= time.Minute {
		p := tokenCache.policy
		tokenCache.mu.RUnlock()
		return p
	}
	tokenCache.mu.RUnlock()

	tokenCache.mu.Lock()
	defer tokenCache.mu.Unlock()
	if !tokenCache.lastLoaded.IsZero() && now.Sub(tokenCache.lastLoaded) <= time.Minute {
		return tokenCache.policy
	}
	tokenCache.policy = mcpPolicy{
		Token:    store.GetSetting(mcpTokenSetting),
		Enabled:  store.GetSetting(mcpEnabledSetting) == "true",
		CIDRs:    store.GetSetting(mcpCIDRSetting),
		ReadOnly: store.GetSetting(mcpReadOnlySetting) != "false",
	}
	tokenCache.lastLoaded = now
	return tokenCache.policy
}

// GetCurrentToken is retained for integrations that need the MCP API token.
func GetCurrentToken() string {
	return loadMCPPolicy(time.Now()).Token
}

func loadPanelAllowedIPs(now time.Time) string {
	tokenCache.mu.RLock()
	if !tokenCache.lastIPLoad.IsZero() && now.Sub(tokenCache.lastIPLoad) <= time.Minute {
		v := tokenCache.allowedIPs
		tokenCache.mu.RUnlock()
		return v
	}
	tokenCache.mu.RUnlock()

	tokenCache.mu.Lock()
	defer tokenCache.mu.Unlock()
	if !tokenCache.lastIPLoad.IsZero() && now.Sub(tokenCache.lastIPLoad) <= time.Minute {
		return tokenCache.allowedIPs
	}
	tokenCache.allowedIPs = store.GetSetting("allowed_ips")
	tokenCache.lastIPLoad = now
	return tokenCache.allowedIPs
}

// ipAllowed accepts a comma-separated list of exact IPs or CIDRs. Empty is
// intentionally configurable by the caller because panel and MCP defaults differ.
func ipAllowed(clientIP, rules string, allowEmpty bool) bool {
	rules = strings.TrimSpace(rules)
	if rules == "" {
		return allowEmpty
	}
	addr, err := netip.ParseAddr(clientIP)
	if err != nil {
		return false
	}
	for _, raw := range strings.Split(rules, ",") {
		rule := strings.TrimSpace(raw)
		if rule == "" {
			continue
		}
		if prefix, err := netip.ParsePrefix(rule); err == nil && prefix.Contains(addr) {
			return true
		}
		if allowedAddr, err := netip.ParseAddr(rule); err == nil && allowedAddr == addr {
			return true
		}
	}
	return false
}

// ValidateIPRules validates the comma-separated IP/CIDR form accepted by both
// panel and MCP policies. Keeping this parser server-side avoids a UI-only gate.
func ValidateIPRules(rules string, allowEmpty bool) error {
	rules = strings.TrimSpace(rules)
	if rules == "" {
		if allowEmpty {
			return nil
		}
		return fmt.Errorf("at least one IP or CIDR is required")
	}
	for _, raw := range strings.Split(rules, ",") {
		rule := strings.TrimSpace(raw)
		if rule == "" {
			continue
		}
		if _, err := netip.ParsePrefix(rule); err == nil {
			continue
		}
		if _, err := netip.ParseAddr(rule); err == nil {
			continue
		}
		return fmt.Errorf("invalid IP or CIDR %q", rule)
	}
	return nil
}

func tokenEqual(a, b string) bool {
	if a == "" || len(a) != len(b) {
		return false
	}
	return subtle.ConstantTimeCompare([]byte(a), []byte(b)) == 1
}

// allowQueryToken is intentionally restricted to browser WebSocket upgrades.
func allowQueryToken(path string) bool {
	return strings.HasPrefix(path, "/api/build/logs/") ||
		strings.HasPrefix(path, "/api/pty/") ||
		strings.HasPrefix(path, "/api/shell/")
}

// mcpEndpointPolicy is an explicit allowlist. MCP never falls through to
// "any GET is fine" — every accessible route is declared here with its
// capability. Write endpoints are only reachable when read-only mode is off.
type mcpEndpoint struct {
	method     string
	prefix     string
	write      bool
}

var mcpAllowlist = []mcpEndpoint{
	{method: http.MethodGet, prefix: "/api/dashboard", write: false},
	{method: http.MethodGet, prefix: "/api/clients", write: false},
	{method: http.MethodGet, prefix: "/api/clients/history/", write: false},
	{method: http.MethodGet, prefix: "/api/listeners", write: false},
	{method: http.MethodGet, prefix: "/api/tunnel", write: false},
	{method: http.MethodGet, prefix: "/api/socks", write: false},
	{method: http.MethodGet, prefix: "/api/files/list", write: false},
	{method: http.MethodGet, prefix: "/api/files/read", write: false},
	{method: http.MethodGet, prefix: "/api/files/download", write: false},
	{method: http.MethodGet, prefix: "/api/processes/list", write: false},
	{method: http.MethodGet, prefix: "/api/plugins", write: false},
	{method: http.MethodGet, prefix: "/api/plugins/result/", write: false},
	{method: http.MethodGet, prefix: "/api/modules", write: false},
	{method: http.MethodGet, prefix: "/api/modules/pack/", write: false},
	{method: http.MethodGet, prefix: "/api/resp", write: false},
	{method: http.MethodPost, prefix: "/api/cmd", write: true},
	{method: http.MethodPost, prefix: "/api/files/delete", write: true},
	{method: http.MethodPost, prefix: "/api/files/upload", write: true},
	{method: http.MethodPost, prefix: "/api/processes/kill", write: true},
	{method: http.MethodPost, prefix: "/api/tunnel/start", write: true},
	{method: http.MethodPost, prefix: "/api/tunnel/stop", write: true},
	{method: http.MethodPost, prefix: "/api/tunnel/delete", write: true},
	{method: http.MethodPost, prefix: "/api/socks/start", write: true},
	{method: http.MethodPost, prefix: "/api/socks/stop", write: true},
	{method: http.MethodPost, prefix: "/api/socks/delete", write: true},
	{method: http.MethodPost, prefix: "/api/plugins/run", write: true},
	{method: http.MethodPost, prefix: "/api/plugins/upload", write: true},
	{method: http.MethodPost, prefix: "/api/modules/push", write: true},
	{method: http.MethodPost, prefix: "/api/modules/query", write: true},
	{method: http.MethodPost, prefix: "/api/modules/upload", write: true},
}

// mcpEndpointAllowed returns (allowed, writeRequested). Unknown endpoints are
// denied by default; read-only mode rejects write endpoints.
func mcpEndpointAllowed(method, path string, readOnly bool) (bool, bool) {
	for _, e := range mcpAllowlist {
		if e.method != method {
			continue
		}
		if strings.HasPrefix(path, e.prefix) {
			if e.write && readOnly {
				return false, true
			}
			return true, e.write
		}
	}
	return false, false
}

func denyMCP(c *gin.Context, code string) {
	log.Printf("[Security] MCP %s denied %s %s — %s", c.ClientIP(), c.Request.Method, c.Request.URL.Path, code)
	c.JSON(http.StatusForbidden, gin.H{"error": "mcp policy denied", "error_code": code})
	c.Abort()
}

func setPrincipal(c *gin.Context, principal Principal) {
	c.Set(principalContextKey, principal)
}

// SetPrincipalForTest injects a principal in unit tests (RBAC route table).
func SetPrincipalForTest(c *gin.Context, principal Principal) {
	setPrincipal(c, principal)
}

func currentPrincipal(c *gin.Context) (Principal, bool) {
	v, ok := c.Get(principalContextKey)
	if !ok {
		return Principal{}, false
	}
	p, ok := v.(Principal)
	return p, ok
}

func isAdminRole(role string) bool {
	switch strings.ToLower(strings.TrimSpace(role)) {
	case "admin", "administrator":
		return true
	default:
		return false
	}
}

// RequireAdmin protects management-plane routes from both ordinary operators
// and MCP credentials, even when MCP write mode is explicitly enabled.
func RequireAdmin() gin.HandlerFunc {
	return func(c *gin.Context) {
		principal, ok := currentPrincipal(c)
		if !ok || principal.Kind != "user" || !isAdminRole(principal.Role) {
			c.JSON(http.StatusForbidden, gin.H{"error": "admin role required"})
			c.Abort()
			return
		}
		c.Next()
	}
}

func AuthMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		path := c.Request.URL.Path
		if path == "/api/auth/login" || strings.HasPrefix(path, "/api/s/") || !strings.HasPrefix(path, "/api") {
			c.Next()
			return
		}

		now := time.Now()
		clientIP := c.ClientIP()
		if !ipAllowed(clientIP, loadPanelAllowedIPs(now), true) {
			log.Printf("[Security] panel access denied for IP %s", clientIP)
			c.Status(http.StatusForbidden)
			c.Abort()
			return
		}

		token := ""
		if authHeader := c.GetHeader("Authorization"); strings.HasPrefix(authHeader, "Bearer ") {
			token = strings.TrimSpace(strings.TrimPrefix(authHeader, "Bearer "))
		}
		if token == "" && allowQueryToken(path) {
			token = strings.TrimSpace(c.Query("token"))
		}
		if token == "" {
			c.Status(http.StatusUnauthorized)
			c.Abort()
			return
		}

		policy := loadMCPPolicy(now)
		if tokenEqual(token, policy.Token) {
			if !policy.Enabled {
				denyMCP(c, "mcp_disabled")
				return
			}
			if !ipAllowed(clientIP, policy.CIDRs, false) {
				log.Printf("[Security] MCP access denied for IP %s", clientIP)
				c.Status(http.StatusForbidden)
				c.Abort()
				return
			}
			allowed, writeRequested := mcpEndpointAllowed(c.Request.Method, path, policy.ReadOnly)
			if !allowed {
				if writeRequested {
					denyMCP(c, "mcp_read_only")
				} else {
					denyMCP(c, "mcp_endpoint_denied")
				}
				return
			}
			setPrincipal(c, Principal{Kind: "mcp", Username: "mcp", Role: "mcp"})
			c.Next()
			return
		}

		var user model.User
		if err := store.DB.Where("token = ?", token).First(&user).Error; err != nil || !user.IsActive {
			log.Printf("[Security] invalid panel token from IP %s", clientIP)
			c.Status(http.StatusUnauthorized)
			c.Abort()
			return
		}
		setPrincipal(c, Principal{Kind: "user", Username: user.Username, Role: user.Role})
		c.Next()
	}
}
