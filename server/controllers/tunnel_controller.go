package controllers

import (
    "cupcake-server/services"
    "github.com/gin-gonic/gin"
	"net/http"
)

// GET /api/socks (Alias for /api/tunnel)
func ListSocks(c *gin.Context) {
    ListTunnels(c)
}

// GET /api/tunnel
func ListTunnels(c *gin.Context) {
    tunnels := services.GetActiveTunnels()
    c.JSON(200, gin.H{"status": "success", "tunnels": tunnels})
}

// POST /api/socks/stop
func StopSocks(c *gin.Context) {
    StopTunnel(c)
}

// POST /api/tunnel/stop
func StopTunnel(c *gin.Context) {
    var req struct {
        Port string `json:"port"`
    }
    if err := c.BindJSON(&req); err != nil { 
        c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid JSON"})
        return 
    }

    if err := services.StopTunnel(req.Port); err != nil {
        c.JSON(400, gin.H{"status": "error", "message": err.Error()})
        return
    }
    c.JSON(200, gin.H{"status": "success", "message": "Tunnel stopped"})
}

func DeleteTunnelController(c *gin.Context) {
    var req struct {
        Port string `json:"port"`
    }
    if err := c.BindJSON(&req); err != nil {
        c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid JSON"})
        return
    }

    if err := services.DeleteTunnel(req.Port); err != nil {
        c.JSON(500, gin.H{"error": err.Error()})
        return
    }
    c.JSON(200, gin.H{"status": "success", "message": "Tunnel deleted"})
}

func StartSocks(c *gin.Context) {
    StartTunnel(c)
}

func StartTunnel(c *gin.Context) {
    var req struct {
        UUID     string `json:"uuid"`
        Port     string `json:"port"`
        Type     string `json:"type"` // "socks5" or "http"
        Username string `json:"username"`
        Password string `json:"password"`
    }
    if err := c.ShouldBindJSON(&req); err != nil { 
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid JSON"})
		return 
	}

    if req.Type == "" { req.Type = "socks5" }

    if err := services.StartTunnel(req.UUID, req.Port, req.Type, req.Username, req.Password); err != nil {
        c.JSON(500, gin.H{"status": "error", "message": err.Error()})
        return
    }
    
    c.JSON(200, gin.H{"status": "success", "message": req.Type + " tunnel started on " + req.Port})
}
