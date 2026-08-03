package main

import (
	"net/http"
	"testing"
	"time"
)

func TestNewAdminHTTPServerTimeouts(t *testing.T) {
	h := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {})
	srv := newAdminHTTPServer("127.0.0.1:0", h)
	if srv.ReadHeaderTimeout != 10*time.Second {
		t.Fatalf("ReadHeaderTimeout: got %v want 10s", srv.ReadHeaderTimeout)
	}
	if srv.ReadTimeout != 60*time.Second {
		t.Fatalf("ReadTimeout: got %v want 60s", srv.ReadTimeout)
	}
	if srv.WriteTimeout != 300*time.Second {
		t.Fatalf("WriteTimeout: got %v want 300s", srv.WriteTimeout)
	}
	if srv.IdleTimeout != 120*time.Second {
		t.Fatalf("IdleTimeout: got %v want 120s", srv.IdleTimeout)
	}
	if srv.MaxHeaderBytes != 1<<20 {
		t.Fatalf("MaxHeaderBytes: got %d want %d", srv.MaxHeaderBytes, 1<<20)
	}
	if srv.Handler == nil {
		t.Fatal("handler must be set")
	}
	if srv.Addr != "127.0.0.1:0" {
		t.Fatalf("Addr: got %q", srv.Addr)
	}
}
