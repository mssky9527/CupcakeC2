package stagerguard

import (
	"os"
	"strconv"
	"sync"
)

// DefaultMaxHits is the default max downloads per stager/stage2 cache id.
const DefaultMaxHits = 5

// MaxHitsFromEnv reads CUPCAKE_STAGER_MAX_HITS (default 5). Values < 1 fall back to default.
func MaxHitsFromEnv() int {
	s := os.Getenv("CUPCAKE_STAGER_MAX_HITS")
	if s == "" {
		return DefaultMaxHits
	}
	n, err := strconv.Atoi(s)
	if err != nil || n < 1 {
		return DefaultMaxHits
	}
	return n
}

// HitCounter tracks download counts per id with a max allowance.
type HitCounter struct {
	mu   sync.Mutex
	hits map[string]int
	max  int
}

// NewHitCounter creates a counter; max < 1 uses DefaultMaxHits.
func NewHitCounter(max int) *HitCounter {
	if max < 1 {
		max = DefaultMaxHits
	}
	return &HitCounter{
		hits: make(map[string]int),
		max:  max,
	}
}

// Max returns the configured max hits.
func (h *HitCounter) Max() int {
	if h == nil {
		return DefaultMaxHits
	}
	return h.max
}

// Try increments the hit count for id and returns true if still within max.
// When exceeded, returns false (count remains > max so subsequent Try also fail).
func (h *HitCounter) Try(id string) bool {
	if h == nil {
		return true
	}
	if id == "" {
		return false
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	h.hits[id]++
	return h.hits[id] <= h.max
}

// Count returns current hits for id (0 if never seen).
func (h *HitCounter) Count(id string) int {
	if h == nil {
		return 0
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.hits[id]
}

// Reset clears the counter for id (e.g. when re-storing a cache entry).
func (h *HitCounter) Reset(id string) {
	if h == nil || id == "" {
		return
	}
	h.mu.Lock()
	delete(h.hits, id)
	h.mu.Unlock()
}

// Delete is an alias for Reset (evict on expiry / max).
func (h *HitCounter) Delete(id string) {
	h.Reset(id)
}
