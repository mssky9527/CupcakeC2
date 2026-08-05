package stagerguard

import (
	"os"
	"testing"
)

func TestHitCounterMax(t *testing.T) {
	h := NewHitCounter(3)
	id := "abc123"
	for i := 0; i < 3; i++ {
		if !h.Try(id) {
			t.Fatalf("hit %d should be allowed", i+1)
		}
	}
	if h.Try(id) {
		t.Fatal("hit 4 should exceed max 3")
	}
	if h.Count(id) != 4 {
		t.Fatalf("count: got %d want 4", h.Count(id))
	}
}

func TestHitCounterReset(t *testing.T) {
	h := NewHitCounter(2)
	id := "x"
	_ = h.Try(id)
	_ = h.Try(id)
	if h.Try(id) {
		t.Fatal("should be maxed")
	}
	h.Reset(id)
	if h.Count(id) != 0 {
		t.Fatalf("after reset count=%d", h.Count(id))
	}
	if !h.Try(id) {
		t.Fatal("after reset should allow")
	}
}

func TestHitCounterEmptyID(t *testing.T) {
	h := NewHitCounter(5)
	if h.Try("") {
		t.Fatal("empty id must fail")
	}
}

func TestMaxHitsFromEnv(t *testing.T) {
	t.Setenv("CUPCAKE_STAGER_MAX_HITS", "")
	if MaxHitsFromEnv() != DefaultMaxHits {
		t.Fatalf("default: got %d", MaxHitsFromEnv())
	}
	t.Setenv("CUPCAKE_STAGER_MAX_HITS", "12")
	if MaxHitsFromEnv() != 12 {
		t.Fatalf("env 12: got %d", MaxHitsFromEnv())
	}
	t.Setenv("CUPCAKE_STAGER_MAX_HITS", "0")
	if MaxHitsFromEnv() != DefaultMaxHits {
		t.Fatalf("invalid 0 should default, got %d", MaxHitsFromEnv())
	}
	t.Setenv("CUPCAKE_STAGER_MAX_HITS", "nope")
	if MaxHitsFromEnv() != DefaultMaxHits {
		t.Fatalf("invalid string should default, got %d", MaxHitsFromEnv())
	}
	_ = os.Unsetenv("CUPCAKE_STAGER_MAX_HITS")
}

func TestNewHitCounterDefaultMax(t *testing.T) {
	h := NewHitCounter(0)
	if h.Max() != DefaultMaxHits {
		t.Fatalf("max: got %d", h.Max())
	}
}
