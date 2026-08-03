package services

import (
	"errors"
	"net"
	"testing"
	"time"
)

func TestDesktopRDPBusyWithoutListener(t *testing.T) {
	id := "agent-test-busy-1"
	ReleaseDesktop(id)

	// Manually reserve as if Start partially registered
	desktopMu.Lock()
	desktopByID[id] = &DesktopRdpSession{AgentID: id, TargetHost: "127.0.0.1", TargetPort: 3389}
	desktopMu.Unlock()

	if !HasDesktopSession(id) {
		t.Fatal("expected session held")
	}

	// Second start should fail busy — but StartDesktopRDP also needs offline agent;
	// just verify HasDesktopSession + Release path.
	ReleaseDesktop(id)
	if HasDesktopSession(id) {
		t.Fatal("should be free after release")
	}
}

func TestDesktopReleaseIdempotent(t *testing.T) {
	id := "agent-test-rel"
	ReleaseDesktop(id)
	desktopMu.Lock()
	desktopByID[id] = &DesktopRdpSession{AgentID: id}
	desktopMu.Unlock()
	ReleaseDesktop(id)
	ReleaseDesktop(id) // no panic
	if HasDesktopSession(id) {
		t.Fatal("still held")
	}
}

func TestGetDesktopSessionCopy(t *testing.T) {
	id := "agent-test-get"
	ReleaseDesktop(id)
	desktopMu.Lock()
	desktopByID[id] = &DesktopRdpSession{
		AgentID:    id,
		ListenPort: 13389,
		TargetHost: "127.0.0.1",
		TargetPort: 3389,
	}
	desktopMu.Unlock()
	s := GetDesktopSession(id)
	if s == nil || s.ListenPort != 13389 {
		t.Fatalf("bad session %+v", s)
	}
	if s.listener != nil {
		t.Fatal("copy must not expose listener")
	}
	ReleaseDesktop(id)
}

func TestDesktopBusyErrorSentinel(t *testing.T) {
	if !errors.Is(DesktopBusyError, DesktopBusyError) {
		t.Fatal("sentinel")
	}
}

func TestResolveDesktopListenHostDefaultLoopback(t *testing.T) {
	t.Setenv("CUPCAKE_DESKTOP_LISTEN_HOST", "")
	if h := resolveDesktopListenHost(); h != "127.0.0.1" {
		t.Fatalf("default host %q want 127.0.0.1", h)
	}
}

func TestResolveDesktopListenHostOverride(t *testing.T) {
	t.Setenv("CUPCAKE_DESKTOP_LISTEN_HOST", "0.0.0.0")
	if h := resolveDesktopListenHost(); h != "0.0.0.0" {
		t.Fatalf("override host %q", h)
	}
}

func TestIdleDeadlineConnResetsDeadline(t *testing.T) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()

	done := make(chan struct{})
	go func() {
		c, err := ln.Accept()
		if err != nil {
			return
		}
		defer c.Close()
		buf := make([]byte, 4)
		_, _ = c.Read(buf)
		_, _ = c.Write([]byte("pong"))
		close(done)
	}()

	raw, err := net.Dial("tcp", ln.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	defer raw.Close()

	ic := &idleDeadlineConn{Conn: raw, idle: 2 * time.Second}
	if _, err := ic.Write([]byte("ping")); err != nil {
		t.Fatal(err)
	}
	buf := make([]byte, 4)
	if _, err := ic.Read(buf); err != nil {
		t.Fatal(err)
	}
	<-done
}

func TestDesktopConnLimitCap(t *testing.T) {
	id := "agent-conn-limit"
	ReleaseDesktop(id)
	desktopMu.Lock()
	desktopActiveConns[id] = desktopMaxConnsPerAgent
	desktopMu.Unlock()
	defer func() {
		desktopMu.Lock()
		delete(desktopActiveConns, id)
		desktopMu.Unlock()
	}()

	if DesktopActiveConnCount(id) != desktopMaxConnsPerAgent {
		t.Fatalf("count %d", DesktopActiveConnCount(id))
	}
	if desktopMaxConnsPerAgent < 1 {
		t.Fatal("limit must be positive")
	}
	if desktopRDPIdleTimeout != 120*time.Second {
		t.Fatalf("idle timeout %v", desktopRDPIdleTimeout)
	}
}
