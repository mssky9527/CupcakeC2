// Package metrics provides lightweight in-process observability counters for
// the admin control plane (no external Prometheus dependency).
package metrics

import (
	"sync/atomic"
	"time"
)

var (
	// processStart is set once at package init for uptime_sec.
	processStart = time.Now()

	// MCPDeniesTotal counts MCP policy denials (endpoint, read-only, disabled, IP).
	MCPDeniesTotal atomic.Int64

	// RBACDeniesTotal counts RequireAdmin / RequireOperator / RequireViewer / RequireRole denials.
	RBACDeniesTotal atomic.Int64
)

// IncMCPDeny increments the MCP deny counter.
func IncMCPDeny() {
	MCPDeniesTotal.Add(1)
}

// IncRBACDeny increments the RBAC deny counter.
func IncRBACDeny() {
	RBACDeniesTotal.Add(1)
}

// UptimeSec returns seconds since process start.
func UptimeSec() int64 {
	return int64(time.Since(processStart).Seconds())
}

// Snapshot returns a copy of current counter values (for handlers/tests).
func Snapshot() (mcpDenies, rbacDenies, uptimeSec int64) {
	return MCPDeniesTotal.Load(), RBACDeniesTotal.Load(), UptimeSec()
}

// ResetForTest zeroes counters (tests only). Does not reset processStart.
func ResetForTest() {
	MCPDeniesTotal.Store(0)
	RBACDeniesTotal.Store(0)
}
