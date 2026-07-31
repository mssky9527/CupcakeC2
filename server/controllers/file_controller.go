package controllers

import (
	"encoding/base64"
	"io"
	"net/http"
	"strconv"

	"cupcake-server/pkg/globals"
	"cupcake-server/services"

	"github.com/gin-gonic/gin"
)

func ReadFileController(c *gin.Context) {
	uuid := c.Query("uuid")
	path := c.Query("path")
	if uuid == "" || path == "" {
		c.JSON(400, gin.H{"error": "uuid and path are required"})
		return
	}

	resp, err := services.ReadFile(uuid, path)
	if err != nil {
		if err.Error() == "offline" {
			c.JSON(http.StatusNotFound, gin.H{"error": "Agent offline"})
		} else {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		}
		return
	}

	if resp.Status == "error" {
		c.JSON(500, gin.H{"error": resp.Error})
		return
	}

	c.JSON(200, resp)
}

type DeleteRequest struct {
	UUID  string   `json:"uuid"`
	Paths []string `json:"paths"`
}

func DeleteFilesController(c *gin.Context) {
	var req DeleteRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(400, gin.H{"error": "invalid request body"})
		return
	}

	if req.UUID == "" || len(req.Paths) == 0 {
		c.JSON(400, gin.H{"error": "uuid and paths are required"})
		return
	}

	resp, err := services.DeleteFiles(req.UUID, req.Paths)
	if err != nil {
		if err.Error() == "offline" {
			c.JSON(http.StatusNotFound, gin.H{"error": "Agent offline"})
		} else {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		}
		return
	}

	if resp.Status == "error" {
		c.JSON(500, gin.H{"error": resp.Error})
		return
	}

	c.JSON(200, gin.H{"status": "ok"})
}

func ListFilesController(c *gin.Context) {
	uuid := c.Query("uuid")
	path := c.Query("path")

	// Compatibility: the frontend calls POST /api/fs/ls with JSON body.
	// Keep GET query support for older clients.
	if uuid == "" && c.Request.Method != http.MethodGet {
		var req struct {
			UUID       string `json:"uuid"`
			AgentUUID  string `json:"agent_uuid"`
			ClientUUID string `json:"client_uuid"`
			Path       string `json:"path"`
			Dir        string `json:"dir"`
		}
		if err := c.ShouldBindJSON(&req); err == nil {
			if req.UUID != "" {
				uuid = req.UUID
			} else if req.AgentUUID != "" {
				uuid = req.AgentUUID
			} else if req.ClientUUID != "" {
				uuid = req.ClientUUID
			}

			if req.Path != "" {
				path = req.Path
			} else if req.Dir != "" {
				path = req.Dir
			}
		}
	}

	if uuid == "" {
		c.JSON(400, gin.H{"error": "uuid is required"})
		return
	}

	resp, err := services.GetFileList(uuid, path)
	if err != nil {
		if err.Error() == "offline" {
			c.JSON(http.StatusNotFound, gin.H{"error": "Agent offline"})
		} else {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		}
		return
	}

	if resp.Status == "error" {
		c.JSON(500, gin.H{"error": resp.Error})
		return
	}

	c.JSON(200, resp)
}

func Upload(c *gin.Context) {
	// Allow larger multipart bodies (default is often too low for multi-MB files).
	_ = c.Request.ParseMultipartForm(64 << 20) // 64 MiB memory budget; rest spilled to temp

	uuid := c.PostForm("uuid")
	targetPath := c.PostForm("path")
	file, err := c.FormFile("file")

	if uuid == "" {
		c.JSON(400, gin.H{"error": "missing form field: uuid"})
		return
	}
	if targetPath == "" {
		c.JSON(400, gin.H{"error": "missing form field: path (remote destination path)"})
		return
	}
	if err != nil {
		// Most common: frontend set Content-Type: multipart/form-data without boundary
		c.JSON(400, gin.H{
			"error": "missing form file field 'file' (or multipart parse failed: " + err.Error() + ")",
		})
		return
	}

	if _, ok := globals.Clients.Load(uuid); !ok {
		c.JSON(404, gin.H{"error": "Agent Offline"})
		return
	}

	f, err := file.Open()
	if err != nil {
		c.JSON(500, gin.H{"error": "Failed to open upload stream: " + err.Error()})
		return
	}
	defer f.Close()

	// 512 KiB raw chunks → ~680 KiB base64 — safer for encrypt/obfuscate frames than 2 MiB.
	const chunkSize = 512 * 1024
	buffer := make([]byte, chunkSize)
	isAppend := false
	var total int64

	for {
		n, readErr := f.Read(buffer)
		if n > 0 {
			b64Data := base64.StdEncoding.EncodeToString(buffer[:n])
			if errSend := services.UploadChunk(uuid, targetPath, b64Data, isAppend); errSend != nil {
				c.JSON(500, gin.H{
					"error": "Agent upload failed at offset " + formatInt64(total) + ": " + errSend.Error(),
				})
				return
			}
			total += int64(n)
			isAppend = true
		}

		if readErr == io.EOF {
			break
		}
		if readErr != nil {
			c.JSON(500, gin.H{"error": "Read stream error: " + readErr.Error()})
			return
		}
	}

	c.JSON(200, gin.H{"status": "upload_success", "bytes": total, "path": targetPath})
}

func formatInt64(n int64) string {
	return strconv.FormatInt(n, 10)
}
