package stagerguard

import (
	"testing"
	"time"
)

func TestFixedWindowLimiterAllow(t *testing.T) {
	lim := NewFixedWindowLimiter(3, time.Minute)
	base := time.Date(2026, 1, 1, 12, 0, 0, 0, time.UTC)
	lim.nowFunc = func() time.Time { return base }

	for i := 0; i < 3; i++ {
		if !lim.Allow("1.2.3.4") {
			t.Fatalf("request %d should be allowed", i+1)
		}
	}
	if lim.Allow("1.2.3.4") {
		t.Fatal("4th request in window should be denied")
	}
	if lim.Count("1.2.3.4") != 4 {
		t.Fatalf("count: got %d want 4", lim.Count("1.2.3.4"))
	}
	// other IP independent
	if !lim.Allow("9.9.9.9") {
		t.Fatal("other IP should be allowed")
	}
}

func TestFixedWindowLimiterResets(t *testing.T) {
	lim := NewFixedWindowLimiter(2, time.Minute)
	base := time.Date(2026, 1, 1, 12, 0, 0, 0, time.UTC)
	now := base
	lim.nowFunc = func() time.Time { return now }

	if !lim.Allow("ip") || !lim.Allow("ip") {
		t.Fatal("first two should pass")
	}
	if lim.Allow("ip") {
		t.Fatal("third should fail")
	}
	// advance past window
	now = base.Add(time.Minute + time.Second)
	if !lim.Allow("ip") {
		t.Fatal("after window reset should allow")
	}
	if lim.Count("ip") != 1 {
		t.Fatalf("count after reset: got %d want 1", lim.Count("ip"))
	}
}

func TestFixedWindowLimiterDisabled(t *testing.T) {
	lim := NewFixedWindowLimiter(0, time.Minute)
	for i := 0; i < 100; i++ {
		if !lim.Allow("x") {
			t.Fatal("limit 0 should never deny")
		}
	}
}

func TestFixedWindowLimiterPurgeExpired(t *testing.T) {
	lim := NewFixedWindowLimiter(5, time.Minute)
	base := time.Date(2026, 1, 1, 12, 0, 0, 0, time.UTC)
	now := base
	lim.nowFunc = func() time.Time { return now }
	_ = lim.Allow("a")
	now = base.Add(2 * time.Minute)
	lim.PurgeExpired()
	if lim.Count("a") != 0 {
		t.Fatalf("purged key should have count 0, got %d", lim.Count("a"))
	}
}
