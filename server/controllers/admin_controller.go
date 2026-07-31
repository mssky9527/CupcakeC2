package controllers

import (
	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/model"
	"cupcake-server/pkg/store"
	"fmt"
	"net/http"
	"sync"
	"time"

	"github.com/gin-gonic/gin"
	"gorm.io/gorm"
	"cupcake-server/services"
)

// loginLock: after 5 failed attempts, lock IP for 5 minutes
const (
	loginMaxFails   = 5
	loginLockWindow = 5 * time.Minute
)

var loginLimiter = struct {
	mu       sync.Mutex
	fails    map[string]int       // consecutive failures
	lockedAt map[string]time.Time // when lock started (zero = not locked)
}{
	fails:    make(map[string]int),
	lockedAt: make(map[string]time.Time),
}

// loginLockRemaining returns remaining lock duration, or 0 if allowed.
func loginLockRemaining(ip string) time.Duration {
	loginLimiter.mu.Lock()
	defer loginLimiter.mu.Unlock()
	now := time.Now()
	if t, ok := loginLimiter.lockedAt[ip]; ok && !t.IsZero() {
		until := t.Add(loginLockWindow)
		if now.Before(until) {
			return until.Sub(now)
		}
		// lock expired
		delete(loginLimiter.lockedAt, ip)
		loginLimiter.fails[ip] = 0
	}
	return 0
}

func recordLoginFailure(ip string) {
	loginLimiter.mu.Lock()
	defer loginLimiter.mu.Unlock()
	loginLimiter.fails[ip]++
	if loginLimiter.fails[ip] >= loginMaxFails {
		loginLimiter.lockedAt[ip] = time.Now()
	}
}

func recordLoginSuccess(ip string) {
	loginLimiter.mu.Lock()
	defer loginLimiter.mu.Unlock()
	loginLimiter.fails[ip] = 0
	delete(loginLimiter.lockedAt, ip)
}

// sensitive settings cannot be written via generic settings API
var blockedSettingKeys = map[string]bool{
	"system_api_token":  true,
	"web_auth_password": true,
	"web_auth_user":     true,
	"admin_pass":        true,
	"admin_password":    true,
}

// HandleLogin handles user authentication (panel form login only)
func HandleLogin(c *gin.Context) {
	ip := c.ClientIP()
	if rem := loginLockRemaining(ip); rem > 0 {
		c.JSON(http.StatusTooManyRequests, gin.H{
			"error":       "too many failed logins, account locked",
			"retry_after": int(rem.Seconds()) + 1,
			"lock_min":    5,
		})
		return
	}

	var req struct {
		Username string `json:"username"`
		Password string `json:"password"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid request"})
		return
	}

	user, err := store.GetUserByUsername(req.Username)
	if err != nil || user == nil || !store.CheckPasswordHash(req.Password, user.Password) {
		recordLoginFailure(ip)
		store.SaveLoginLog(&model.LoginLog{
			Username:  req.Username,
			IP:        ip,
			UserAgent: c.GetHeader("User-Agent"),
			Status:    "failed",
			Message:   "Invalid credentials",
		})
		// If just locked, tell client
		if rem := loginLockRemaining(ip); rem > 0 {
			c.JSON(http.StatusTooManyRequests, gin.H{
				"error":       "too many failed logins, locked for 5 minutes",
				"retry_after": int(rem.Seconds()) + 1,
			})
			return
		}
		c.JSON(http.StatusUnauthorized, gin.H{"error": "Invalid username or password"})
		return
	}

	if !user.IsActive {
		c.JSON(http.StatusForbidden, gin.H{"error": "Account is disabled"})
		return
	}

	recordLoginSuccess(ip)
	store.SaveLoginLog(&model.LoginLog{
		Username:  req.Username,
		IP:        ip,
		UserAgent: c.GetHeader("User-Agent"),
		Status:    "success",
	})

	// Generate a unique session token for this user login
	sessionToken := store.GenerateSecureToken(32)
	user.Token = sessionToken
	store.SaveUser(user)

	c.JSON(http.StatusOK, gin.H{
		"token": sessionToken,
		"user": gin.H{
			"id":       user.ID,
			"username": user.Username,
			"role":     user.Role,
		},
	})
}

// HandleGetUsers returns all operators
func HandleGetUsers(c *gin.Context) {
	users, err := store.GetAllUsers()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, users)
}

// HandleAddUser creates a new operator
func HandleAddUser(c *gin.Context) {
	var user model.User
	if err := c.ShouldBindJSON(&user); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	hashed, _ := store.HashPassword(user.Password)
	user.Password = hashed

	if err := store.SaveUser(&user); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, user)
}

// HandleUpdateUser updates an existing operator's password or role
func HandleUpdateUser(c *gin.Context) {
	var req struct {
		Password string `json:"password"`
		Role     string `json:"role"`
		IsActive *bool  `json:"is_active"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	idStr := c.Param("id")
	var user model.User
	if err := store.DB.First(&user, idStr).Error; err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "User not found"})
		return
	}

	if req.Password != "" {
		user.Password, _ = store.HashPassword(req.Password)
	}
	if req.Role != "" {
		user.Role = req.Role
	}
	if req.IsActive != nil {
		user.IsActive = *req.IsActive
	}

	store.SaveUser(&user)
	c.JSON(http.StatusOK, gin.H{"msg": "User updated"})
}

// HandleDeleteUser removes an operator
func HandleDeleteUser(c *gin.Context) {
	idStr := c.Param("id")
	var id uint
	fmt.Sscanf(idStr, "%d", &id)
	if err := store.DeleteUser(id); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"msg": "User deleted"})
}

// HandleGetLoginLogs returns recent audit logs
func HandleGetLoginLogs(c *gin.Context) {
	logs, err := store.GetLoginLogs(100)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, logs)
}

// HandleGetSettings returns global config
func HandleGetSettings(c *gin.Context) {
	group := c.Query("group")
	if group != "" {
		settings, _ := store.GetSettingsByGroup(group)
		c.JSON(http.StatusOK, settings)
	} else {
		var settings []model.GlobalSetting
		store.DB.Find(&settings)
		c.JSON(http.StatusOK, settings)
	}
}

// HandleUpdateSettings updates global config (whitelist: rejects sensitive keys)
func HandleUpdateSettings(c *gin.Context) {
	var settings []model.GlobalSetting
	if err := c.ShouldBindJSON(&settings); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	for _, s := range settings {
		if blockedSettingKeys[s.Key] {
			c.JSON(http.StatusForbidden, gin.H{
				"error": fmt.Sprintf("setting key %q cannot be updated via generic endpoint", s.Key),
			})
			return
		}
		store.SetSetting(s.Key, s.Value, s.Group)
	}
	c.JSON(http.StatusOK, gin.H{"msg": "Settings updated"})
}

// HandleGetWebhooks returns all notification hooks
func HandleGetWebhooks(c *gin.Context) {
	hooks, _ := store.GetAllWebhooks()
	c.JSON(http.StatusOK, hooks)
}

// HandleSaveWebhook creates or updates a hook
func HandleSaveWebhook(c *gin.Context) {
	var hook model.NotificationWebhook
	if err := c.ShouldBindJSON(&hook); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	store.SaveWebhook(&hook)
	c.JSON(http.StatusOK, hook)
}

// HandleDeleteWebhook removes a hook
func HandleDeleteWebhook(c *gin.Context) {
	idStr := c.Param("id")
	var id uint
	fmt.Sscanf(idStr, "%d", &id)
	store.DeleteWebhook(id)
	c.JSON(http.StatusOK, gin.H{"msg": "Webhook deleted"})
}

// HandleMaintenanceReset clears sensitive history (admin role + confirmation required)
func HandleMaintenanceReset(c *gin.Context) {
	var req struct {
		Confirm string `json:"confirm"`
	}
	_ = c.ShouldBindJSON(&req)
	confirm := req.Confirm
	if confirm == "" {
		confirm = c.GetHeader("X-Confirm-Reset")
	}
	if confirm != "RESET_ALL_AGENTS" {
		c.JSON(http.StatusBadRequest, gin.H{
			"error": `confirmation required: body {"confirm":"RESET_ALL_AGENTS"} or header X-Confirm-Reset`,
		})
		return
	}

	// Prefer admin role from bearer user session
	authHeader := c.GetHeader("Authorization")
	token := ""
	if len(authHeader) > 7 && authHeader[:7] == "Bearer " {
		token = authHeader[7:]
	}
	if token != "" {
		var user model.User
		if err := store.DB.Where("token = ?", token).First(&user).Error; err == nil {
			if user.Role != "admin" && user.Role != "Admin" && user.Role != "administrator" {
				c.JSON(http.StatusForbidden, gin.H{"error": "admin role required for maintenance reset"})
				return
			}
		}
		// Master API token (MCP) is treated as admin
	}

	store.DB.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&model.Agent{})
	store.DB.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&model.CommandLog{})

	globals.Clients.Range(func(key, value interface{}) bool {
		client := value.(*globals.Client)
		client.CloseOutputChannel()
		globals.Clients.Delete(key)
		return true
	})

	c.JSON(http.StatusOK, gin.H{"msg": "Database reset successful (Agents and Logs cleared)"})
}

// HandleLogout invalidates the current user session token.
func HandleLogout(c *gin.Context) {
	authHeader := c.GetHeader("Authorization")
	if len(authHeader) > 7 && authHeader[:7] == "Bearer " {
		token := authHeader[7:]
		var user model.User
		if err := store.DB.Where("token = ?", token).First(&user).Error; err == nil {
			user.Token = ""
			_ = store.SaveUser(&user)
		}
	}
	c.JSON(http.StatusOK, gin.H{"msg": "logged out"})
}

// HandleMaintenanceExport exports all data
func HandleMaintenanceExport(c *gin.Context) {
	var agents []model.Agent
	var logs []model.CommandLog
	store.DB.Find(&agents)
	store.DB.Find(&logs)

	exportData := gin.H{
		"agents":      agents,
		"logs":        logs,
		"export_time": time.Now(),
	}

	c.Header("Content-Disposition", "attachment; filename=cupcake_export.json")
	c.JSON(http.StatusOK, exportData)
}

// HandleUpdateTemplates triggers a rebuild of the v3.0.1 loader templates
func HandleUpdateTemplates(c *gin.Context) {
	logChan := make(chan string, 50)
	var logs []string
	
	// Collect logs in a separate goroutine
	done := make(chan bool)
	go func() {
		for l := range logChan {
			logs = append(logs, l)
		}
		done <- true
	}()

	err := services.RebuildTemplates(logChan)
	close(logChan)
	<-done

	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{
			"status": "error",
			"error":  err.Error(),
			"logs":   logs,
		})
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"status": "success",
		"msg":    "v3.0.1 模板集更新完成",
		"logs":   logs,
	})
}
