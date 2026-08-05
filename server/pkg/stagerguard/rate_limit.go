package stagerguard

import (
	"sync"
	"time"
)

// FixedWindowLimiter counts requests per key (typically client IP) in fixed windows.
// Safe for concurrent use.
type FixedWindowLimiter struct {
	mu      sync.Mutex
	limit   int
	window  time.Duration
	entries map[string]*windowEntry
	// nowFunc allows tests to control time; nil → time.Now
	nowFunc func() time.Time
}

type windowEntry struct {
	start time.Time
	count int
}

// NewFixedWindowLimiter creates a limiter allowing `limit` events per `window` duration.
// limit <= 0 disables limiting (Allow always true).
func NewFixedWindowLimiter(limit int, window time.Duration) *FixedWindowLimiter {
	if window <= 0 {
		window = time.Minute
	}
	return &FixedWindowLimiter{
		limit:   limit,
		window:  window,
		entries: make(map[string]*windowEntry),
	}
}

func (l *FixedWindowLimiter) now() time.Time {
	if l.nowFunc != nil {
		return l.nowFunc()
	}
	return time.Now()
}

// Allow reports whether key may proceed and records the attempt when allowed.
// When limit is exceeded, the counter is still incremented (standard fixed-window).
func (l *FixedWindowLimiter) Allow(key string) bool {
	if l == nil || l.limit <= 0 {
		return true
	}
	if key == "" {
		key = "unknown"
	}
	now := l.now()
	l.mu.Lock()
	defer l.mu.Unlock()

	e, ok := l.entries[key]
	if !ok || now.Sub(e.start) >= l.window {
		l.entries[key] = &windowEntry{start: now, count: 1}
		return true
	}
	e.count++
	return e.count <= l.limit
}

// Remaining returns how many requests key has left in the current window (for tests/diagnostics).
func (l *FixedWindowLimiter) Remaining(key string) int {
	if l == nil || l.limit <= 0 {
		return l.limit
	}
	if key == "" {
		key = "unknown"
	}
	now := l.now()
	l.mu.Lock()
	defer l.mu.Unlock()
	e, ok := l.entries[key]
	if !ok || now.Sub(e.start) >= l.window {
		return l.limit
	}
	left := l.limit - e.count
	if left < 0 {
		return 0
	}
	return left
}

// PurgeExpired drops windows older than the configured duration (call periodically).
func (l *FixedWindowLimiter) PurgeExpired() {
	if l == nil {
		return
	}
	now := l.now()
	l.mu.Lock()
	defer l.mu.Unlock()
	for k, e := range l.entries {
		if now.Sub(e.start) >= l.window {
			delete(l.entries, k)
		}
	}
}

// Count returns current window count for key (0 if no window); for tests.
func (l *FixedWindowLimiter) Count(key string) int {
	if l == nil {
		return 0
	}
	if key == "" {
		key = "unknown"
	}
	now := l.now()
	l.mu.Lock()
	defer l.mu.Unlock()
	e, ok := l.entries[key]
	if !ok || now.Sub(e.start) >= l.window {
		return 0
	}
	return e.count
}
