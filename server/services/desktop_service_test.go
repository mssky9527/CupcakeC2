package services

import (
	"errors"
	"testing"
	"time"

	"cupcake-server/pkg/utils"
)

func TestDesktopSingleSessionBusy(t *testing.T) {
	id := "agent-test-busy-1"
	ReleaseDesktop(id)

	rel1, err := TryReserveDesktop(id)
	if err != nil {
		t.Fatal(err)
	}
	_, err = TryReserveDesktop(id)
	if !errors.Is(err, DesktopBusyError) {
		t.Fatalf("want busy got %v", err)
	}
	// simulate open failed → release placeholder
	rel1()
	if HasDesktopSession(id) {
		t.Fatal("should be free after release without stream")
	}

	// second reserve ok
	rel2, err := TryReserveDesktop(id)
	if err != nil {
		t.Fatal(err)
	}
	rel2()
	ReleaseDesktop(id)
}

func TestDesktopReleaseIdempotent(t *testing.T) {
	id := "agent-test-rel"
	ReleaseDesktop(id)
	_, err := TryReserveDesktop(id)
	if err != nil {
		t.Fatal(err)
	}
	ReleaseDesktop(id)
	ReleaseDesktop(id) // no panic
	if HasDesktopSession(id) {
		t.Fatal("still held")
	}
}

func TestDesktopAllowFrameUsesBucket(t *testing.T) {
	id := "agent-test-rate"
	ReleaseDesktop(id)
	_, err := TryReserveDesktop(id)
	if err != nil {
		t.Fatal(err)
	}
	// force tiny bucket
	desktopMu.Lock()
	s := desktopByID[id]
	s.Bucket = utils.NewTokenBucket(100, time.Now().UnixMilli())
	s.MaxFrameB = 50
	desktopMu.Unlock()

	if DesktopAllowFrame(id, 60) {
		t.Fatal("over max frame bytes")
	}
	if !DesktopAllowFrame(id, 40) {
		t.Fatal("should allow small")
	}
	if !DesktopAllowFrame(id, 40) {
		t.Fatal("second small frame still under bucket")
	}
	// 20 tokens left; 40 must fail
	if DesktopAllowFrame(id, 40) {
		t.Fatal("bucket should be exhausted")
	}
	ReleaseDesktop(id)
}

func TestSecondReserveIsBusyWithoutYamux(t *testing.T) {
	// Second TryReserve fails before any Yamux Open (no agent required).
	id := "agent-no-yamux-needed"
	ReleaseDesktop(id)
	_, err := TryReserveDesktop(id)
	if err != nil {
		t.Fatal(err)
	}
	_, err = TryReserveDesktop(id)
	if !errors.Is(err, DesktopBusyError) {
		t.Fatalf("want DesktopBusyError got %v", err)
	}
	ReleaseDesktop(id)
}
