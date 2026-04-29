package controllers

import (
	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/model"
	"cupcake-server/pkg/store"
	"fmt"
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
	"gorm.io/gorm"
	"cupcake-server/services"
)

// HandleLogin handles user authentication
func HandleLogin(c *gin.Context) {
	var req struct {
		Username string `json:"username"`
		Password string `json:"password"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid request"})
		return
	}

	user, err := store.GetUserByUsername(req.Username)
	if err != nil || !store.CheckPasswordHash(req.Password, user.Password) {
		store.SaveLoginLog(&model.LoginLog{
			Username:  req.Username,
			IP:        c.ClientIP(),
			UserAgent: c.GetHeader("User-Agent"),
			Status:    "failed",
			Message:   "Invalid credentials",
		})
		c.JSON(http.StatusUnauthorized, gin.H{"error": "Invalid username or password"})
		return
	}

	if !user.IsActive {
		c.JSON(http.StatusForbidden, gin.H{"error": "Account is disabled"})
		return
	}

	store.SaveLoginLog(&model.LoginLog{
		Username:  req.Username,
		IP:        c.ClientIP(),
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

// HandleUpdateSettings updates global config
func HandleUpdateSettings(c *gin.Context) {
	var settings []model.GlobalSetting
	if err := c.ShouldBindJSON(&settings); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	for _, s := range settings {
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

// HandleMaintenanceReset clears sensitive history
func HandleMaintenanceReset(c *gin.Context) {
	store.DB.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&model.Agent{})
	store.DB.Session(&gorm.Session{AllowGlobalUpdate: true}).Delete(&model.CommandLog{})

	globals.Clients.Range(func(key, value interface{}) bool {
		client := value.(*globals.Client)
		// 🛡️ 正确清理：先关闭 OutputChannel，防止 goroutine 泄漏
		if client.OutputChannel != nil {
			// 用 recover 保护，避免重复关闭 panic
			func() {
				defer func() { recover() }()
				close(client.OutputChannel)
			}()
		}
		globals.Clients.Delete(key)
		return true
	})

	c.JSON(http.StatusOK, gin.H{"msg": "Database reset successful (Agents and Logs cleared)"})
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
