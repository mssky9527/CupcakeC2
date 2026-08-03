package middleware

import (
	"net/http"
	"testing"
)

func TestMCPEndpointAllowlistReadOnly(t *testing.T) {
	cases := []struct {
		method   string
		path     string
		readonly bool
		allowed  bool
	}{
		// Read endpoints allowed in read-only mode
		{http.MethodGet, "/api/dashboard", true, true},
		{http.MethodGet, "/api/clients", true, true},
		{http.MethodGet, "/api/clients/history/abc", true, true},
		{http.MethodGet, "/api/listeners", true, true},
		{http.MethodGet, "/api/files/list", true, true},
		{http.MethodGet, "/api/files/read", true, true},
		{http.MethodGet, "/api/processes/list", true, true},
		{http.MethodGet, "/api/plugins", true, true},
		{http.MethodGet, "/api/modules", true, true},
		{http.MethodGet, "/api/resp", true, true},

		// Write endpoints denied in read-only mode
		{http.MethodPost, "/api/cmd", true, false},
		{http.MethodPost, "/api/files/delete", true, false},
		{http.MethodPost, "/api/files/upload", true, false},
		{http.MethodPost, "/api/processes/kill", true, false},
		{http.MethodPost, "/api/tunnel/start", true, false},
		{http.MethodPost, "/api/tunnel/stop", true, false},
		{http.MethodPost, "/api/plugins/run", true, false},
		{http.MethodPost, "/api/plugins/upload", true, false},
		{http.MethodPost, "/api/modules/push", true, false},

		// Write endpoints allowed when read-only is off
		{http.MethodPost, "/api/cmd", false, true},
		{http.MethodPost, "/api/files/delete", false, true},
		{http.MethodPost, "/api/processes/kill", false, true},

		// Unknown endpoints always denied
		{http.MethodGet, "/api/settings/config", true, false},
		{http.MethodGet, "/api/maintenance/export", true, false},
		{http.MethodGet, "/api/auth/login", true, false},
		{http.MethodGet, "/api/unknown", true, false},
		{http.MethodPost, "/api/unknown", false, false},

		// DELETE on modules (admin-only, not in MCP allowlist)
		{http.MethodDelete, "/api/modules/abc", false, false},
		{http.MethodDelete, "/api/listeners/abc", false, false},
	}

	for _, tc := range cases {
		allowed, _ := mcpEndpointAllowed(tc.method, tc.path, tc.readonly)
		if allowed != tc.allowed {
			t.Errorf("mcpEndpointAllowed(%s %s readonly=%v) = %v, want %v",
				tc.method, tc.path, tc.readonly, allowed, tc.allowed)
		}
	}
}

func TestMCPEndpointAllowlistDeniesManagementPaths(t *testing.T) {
	// Even with read-only off, management paths are not in the MCP allowlist.
	managementPaths := []struct {
		method string
		path   string
	}{
		{http.MethodGet, "/api/settings/users"},
		{http.MethodPost, "/api/settings/users"},
		{http.MethodPost, "/api/settings/config"},
		{http.MethodPost, "/api/maintenance/reset"},
		{http.MethodGet, "/api/maintenance/export"},
		{http.MethodPost, "/api/auth/login"},
		{http.MethodPut, "/api/auth/password"},
		{http.MethodPost, "/api/generate"},
		{http.MethodGet, "/api/generate/stream"},
		{http.MethodGet, "/api/stager"},
		{http.MethodPost, "/api/agents/connect"},
	}

	for _, p := range managementPaths {
		allowed, _ := mcpEndpointAllowed(p.method, p.path, false)
		if allowed {
			t.Errorf("management path %s %s must never be MCP-accessible",
				p.method, p.path)
		}
	}
}
