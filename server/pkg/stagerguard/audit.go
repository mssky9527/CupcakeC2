package stagerguard

import "cupcake-server/pkg/logx"

// AuditStatus values for public stager delivery hits.
const (
	StatusOK      = "ok"
	StatusNotFound = "404"
	StatusRateLimit = "429"
	StatusExpired = "expired"
	StatusMaxHits = "max_hits"
	StatusBadID   = "bad_id"
)

// Audit logs one public stager access event (IP, path, id, status).
func Audit(ip, path, id, status string) {
	logx.Info("stager_public_access",
		"ip", ip,
		"path", path,
		"id", id,
		"status", status,
	)
}
