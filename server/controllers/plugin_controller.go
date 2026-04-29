package controllers

import (
	"cupcake-server/services"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"time"
	"strings"

	"github.com/gin-gonic/gin"
)

func HandleListPlugins(c *gin.Context) {
	plugins, err := services.LoadPluginManifest()
	if err != nil {
		c.JSON(500, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, plugins)
}

func HandleRunPlugin(c *gin.Context) {
	var req struct {
		UUID      string `json:"uuid"`
		AgentID   string `json:"agent_id"` // Support both uuid and agent_id
		PluginID  string `json:"plugin_id"`
		Args      string `json:"args"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(400, gin.H{"error": "Invalid input"})
		return
	}

	// Use AgentID if UUID is empty
	targetUUID := strings.TrimSpace(req.UUID)
	if targetUUID == "" {
		targetUUID = strings.TrimSpace(req.AgentID)
	}

	if targetUUID == "" {
		c.JSON(400, gin.H{"error": "uuid or agent_id is required"})
		return
	}

	fmt.Printf("[Debug] Running plugin %s on agent %s\n", req.PluginID, targetUUID)

	taskID, err := services.DeployPlugin(targetUUID, req.PluginID, req.Args)
	if err != nil {
		c.JSON(500, gin.H{"error": err.Error()})
		return
	}

	c.JSON(http.StatusOK, gin.H{"status": "success", "task_id": taskID})
}

func HandleUploadPlugin(c *gin.Context) {
	file, err := c.FormFile("file")
	if err != nil {
		c.JSON(400, gin.H{"error": "File is required"})
		return
	}

	pluginID := c.PostForm("id")
	name := c.PostForm("name")
	desc := c.PostForm("description")
	execType := c.PostForm("type")
	osReq := c.PostForm("required_os")
	category := c.PostForm("category")

	if pluginID == "" { pluginID = fmt.Sprintf("PL-%d", time.Now().Unix()) }

	os.MkdirAll("assets/plugins", 0755)
	savePath := filepath.Join("assets/plugins", file.Filename)
	if err := c.SaveUploadedFile(file, savePath); err != nil {
		c.JSON(500, gin.H{"error": "Failed to save file"})
		return
	}

	manifest := services.PluginMetadata{
		ID:          pluginID,
		Name:        name,
		Description: desc,
		FileName:    file.Filename,
		Type:        execType,
		RequiredOS:  osReq,
		Category:    category,
	}

	if err := services.AddPluginToManifest(manifest); err != nil {
		c.JSON(500, gin.H{"error": "Failed to update manifest"})
		return
	}

	c.JSON(http.StatusOK, gin.H{"status": "success", "plugin": manifest})
}

func HandleDeletePlugin(c *gin.Context) {
	pluginID := c.Param("id")
	fileName, err := services.RemovePluginFromManifest(pluginID)
	if err != nil {
		c.JSON(500, gin.H{"error": err.Error()})
		return
	}
	if fileName != "" {
		os.Remove(filepath.Join("assets/plugins", fileName))
	}
	c.JSON(http.StatusOK, gin.H{"status": "success"})
}

func HandleGetPluginResult(c *gin.Context) {
	taskID := c.Param("task_id")
	logPath := filepath.Join("storage/logs", fmt.Sprintf("task_%s.txt", taskID))
	data, err := os.ReadFile(logPath)
	if err != nil {
		c.JSON(404, gin.H{"error": "Not found"})
		return
	}
	c.String(200, string(data))
}
