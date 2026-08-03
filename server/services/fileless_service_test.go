package services

import (
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

// Donut-linked conversion tests live in fileless_donut_test.go (!nodonut).
// Default / safe suite (-tags nodonut) only exercises cache + input validation.

func TestBuildFilelessStage2RejectsGarbage(t *testing.T) {
	if _, err := BuildFilelessStage2(nil, "x64"); err == nil {
		t.Fatal("expected error")
	}
	if _, err := BuildFilelessStage2([]byte("MZ"), "x64"); err == nil {
		t.Fatal("expected error for tiny buffer")
	}
	// Non-PE should fail before or at converter (stub or real donut)
	if _, err := BuildFilelessStage2([]byte("not-a-pe-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"), "x64"); err == nil {
		t.Fatal("expected reject non-PE")
	}
}

func TestModuleDescribeLoadMode(t *testing.T) {
	_, _, _, mode := ModuleDescribeEx("iso_host")
	if mode != "iso" {
		t.Fatalf("iso_host load_mode=%s", mode)
	}
	_, _, kind, mode := ModuleDescribeEx("desktop")
	if mode != "mem" || kind != "runtime" {
		t.Fatalf("desktop kind=%s mode=%s", kind, mode)
	}
	name, _, kind, mode := ModuleDescribeEx("inject")
	if mode != "mem" || kind != "runtime" {
		t.Fatalf("inject name=%s kind=%s mode=%s", name, kind, mode)
	}
	// Non-product ids are legacy/ignored
	_, _, kind, _ = ModuleDescribeEx("shell")
	if kind != "legacy" {
		t.Fatalf("shell should be legacy kind, got %s", kind)
	}
}
