package controllers

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"

	"cupcake-server/pkg/middleware"
)

// setupRBACRouter mirrors production admin gating for critical routes.
func setupRBACRouter() *gin.Engine {
	gin.SetMode(gin.TestMode)
	r := gin.New()
	// Inject principal from header for tests (X-Test-Role)
	r.Use(func(c *gin.Context) {
		role := c.GetHeader("X-Test-Role")
		if role == "" {
			role = "operator"
		}
		middleware.SetPrincipalForTest(c, middleware.Principal{Kind: "user", Username: "t", Role: role})
		c.Next()
	})
	api := r.Group("/api")
	{
		api.POST("/listeners", middleware.RequireAdmin(), func(c *gin.Context) {
			c.JSON(200, gin.H{"ok": true})
		})
		api.GET("/maintenance/export", middleware.RequireAdmin(), func(c *gin.Context) {
			c.JSON(200, gin.H{"ok": true})
		})
		api.POST("/maintenance/update_templates", middleware.RequireAdmin(), func(c *gin.Context) {
			c.JSON(200, gin.H{"ok": true})
		})
		api.POST("/maintenance/reset", middleware.RequireAdmin(), func(c *gin.Context) {
			c.JSON(200, gin.H{"ok": true})
		})
		// Plugin management plane — same RequireAdmin gate as production main.go
		api.POST("/plugins/upload", middleware.RequireAdmin(), func(c *gin.Context) {
			c.JSON(200, gin.H{"ok": true})
		})
		api.POST("/plugins/run", middleware.RequireAdmin(), func(c *gin.Context) {
			c.JSON(200, gin.H{"ok": true})
		})
		api.DELETE("/plugins/:id", middleware.RequireAdmin(), func(c *gin.Context) {
			c.JSON(200, gin.H{"ok": true})
		})
	}
	return r
}

func TestOperatorForbiddenOnAdminRoutes(t *testing.T) {
	r := setupRBACRouter()
	paths := []struct {
		method string
		path   string
	}{
		{"POST", "/api/listeners"},
		{"GET", "/api/maintenance/export"},
		{"POST", "/api/maintenance/update_templates"},
		{"POST", "/api/maintenance/reset"},
		{"POST", "/api/plugins/upload"},
		{"POST", "/api/plugins/run"},
		{"DELETE", "/api/plugins/x"},
	}
	for _, p := range paths {
		req := httptest.NewRequest(p.method, p.path, nil)
		req.Header.Set("X-Test-Role", "operator")
		w := httptest.NewRecorder()
		r.ServeHTTP(w, req)
		if w.Code != http.StatusForbidden {
			t.Fatalf("%s %s: operator want 403 got %d body=%s", p.method, p.path, w.Code, w.Body.String())
		}
	}
}

func TestAdminAllowedOnAdminRoutes(t *testing.T) {
	r := setupRBACRouter()
	req := httptest.NewRequest("GET", "/api/maintenance/export", nil)
	req.Header.Set("X-Test-Role", "admin")
	w := httptest.NewRecorder()
	r.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("admin want 200 got %d", w.Code)
	}
}
