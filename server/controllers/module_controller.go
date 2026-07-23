package controllers

import (
	"net/http"
	"path/filepath"
	"strings"
	"time"

	"cupcake-server/services"
	"github.com/gin-gonic/gin"
)

// HandleListModules GET /api/modules?uuid= optional agent for loaded_on_agent flags
func HandleListModules(c *gin.Context) {
	ms := services.GetModuleService()
	agentUUID := strings.TrimSpace(c.Query("uuid"))
	catalog := ms.ListCatalog(agentUUID)
	c.JSON(http.StatusOK, gin.H{
		"modules": catalog,
		// backward-compatible id list
		"ids": ms.List(),
	})
}

// HandleUploadModule POST /api/modules/upload
// form: id=iso_host, file=<exe/dll>
func HandleUploadModule(c *gin.Context) {
	id := c.PostForm("id")
	if id == "" {
		id = c.Query("id")
	}
	if id == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "missing module id"})
		return
	}
	file, err := c.FormFile("file")
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "missing file"})
		return
	}
	tmp := filepath.Join("storage", "modules", "_upload_"+filepath.Base(file.Filename))
	if err := c.SaveUploadedFile(file, tmp); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	ms := services.GetModuleService()
	if err := ms.LoadFromFile(id, tmp); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	name, desc, kind := services.ModuleDescribe(id)
	c.JSON(http.StatusOK, gin.H{
		"msg":         "module registered",
		"id":          id,
		"name":        name,
		"description": desc,
		"kind":        kind,
	})
}

// HandlePushModule POST /api/modules/push
// json: {"uuid":"...","id":"iso_host"}
// Waits for agent ack (up to 25s) so UI can show real success / loaded state.
func HandlePushModule(c *gin.Context) {
	var req struct {
		UUID string `json:"uuid"`
		ID   string `json:"id"`
	}
	if err := c.ShouldBindJSON(&req); err != nil || req.UUID == "" || req.ID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "uuid and id required"})
		return
	}
	ms := services.GetModuleService()
	if ms.AgentHasModule(req.UUID, req.ID) {
		name, _, _ := services.ModuleDescribe(req.ID)
		c.JSON(http.StatusOK, gin.H{
			"msg":    "module already staged/loaded on agent",
			"id":     req.ID,
			"name":   name,
			"loaded": true,
			"alive":  true,
		})
		return
	}

	out, err := services.SendModuleStageWait(req.UUID, req.ID, 25*time.Second)
	name, desc, kind := services.ModuleDescribe(req.ID)
	if err != nil {
		// timeout still marks optimistic loaded in SendModuleStageWait
		if strings.Contains(err.Error(), "timeout") {
			c.JSON(http.StatusOK, gin.H{
				"msg":         "模块已下发（等待确认超时，已标记为已推送）",
				"id":          req.ID,
				"name":        name,
				"description": desc,
				"kind":        kind,
				"loaded":      true,
				"alive":       true,
				"warning":     err.Error(),
				"detail":      out,
			})
			return
		}
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error(), "id": req.ID, "name": name})
		return
	}
	c.JSON(http.StatusOK, gin.H{
		"msg":         "模块推送成功，已在目标主机就绪",
		"id":          req.ID,
		"name":        name,
		"description": desc,
		"kind":        kind,
		"loaded":      true,
		"alive":       true,
		"detail":      out,
	})
}

// HandlePackModule GET /api/modules/pack/:id
func HandlePackModule(c *gin.Context) {
	id := c.Param("id")
	ms := services.GetModuleService()
	b64, err := ms.PackBase64(id)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": err.Error()})
		return
	}
	name, desc, kind := services.ModuleDescribe(id)
	c.JSON(http.StatusOK, gin.H{
		"id":          id,
		"name":        name,
		"description": desc,
		"kind":        kind,
		"data":        b64,
	})
}

// HandleQueryAgentModules POST /api/modules/query
func HandleQueryAgentModules(c *gin.Context) {
	var req struct {
		UUID string `json:"uuid"`
	}
	if err := c.ShouldBindJSON(&req); err != nil || req.UUID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "uuid required"})
		return
	}
	out, err := services.SendModuleList(req.UUID)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	ms := services.GetModuleService()
	c.JSON(http.StatusOK, gin.H{
		"result":  out,
		"modules": ms.ListCatalog(req.UUID),
	})
}
