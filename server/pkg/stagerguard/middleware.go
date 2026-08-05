package stagerguard

import (
	"net/http"
	"sync"
	"time"

	"github.com/gin-gonic/gin"
)

const (
	// DefaultRateLimit is max public stager requests per IP per window.
	DefaultRateLimit = 30
	// DefaultRateWindow is the fixed window length for rate limiting.
	DefaultRateWindow = time.Minute
)

var (
	globalOnce    sync.Once
	globalLimiter *FixedWindowLimiter
)

// DefaultLimiter returns the process-wide public stager rate limiter (30/min per IP).
func DefaultLimiter() *FixedWindowLimiter {
	globalOnce.Do(func() {
		globalLimiter = NewFixedWindowLimiter(DefaultRateLimit, DefaultRateWindow)
		go func() {
			t := time.NewTicker(5 * time.Minute)
			defer t.Stop()
			for range t.C {
				globalLimiter.PurgeExpired()
			}
		}()
	})
	return globalLimiter
}

// RateLimitMiddleware rejects with 429 when the client IP exceeds the stager rate limit.
// On 429 it audits and aborts; callers still handle id-level max-hits themselves.
func RateLimitMiddleware() gin.HandlerFunc {
	lim := DefaultLimiter()
	return func(c *gin.Context) {
		ip := c.ClientIP()
		if !lim.Allow(ip) {
			id := c.Param("id")
			Audit(ip, c.Request.URL.Path, id, StatusRateLimit)
			c.Header("Retry-After", "60")
			c.Data(http.StatusTooManyRequests, "text/plain", []byte("rate limit exceeded"))
			c.Abort()
			return
		}
		c.Next()
	}
}
