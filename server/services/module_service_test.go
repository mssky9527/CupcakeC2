package services

import (
	"bytes"
	"testing"
)

func TestPackModuleRoundtrip(t *testing.T) {
	key := DefaultModuleKey()
	if len(key) != 32 {
		t.Fatalf("key len %d", len(key))
	}
	payload := []byte("hello-module-payload")
	blob, err := PackModule("shell", payload, key)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.HasPrefix(blob, []byte("CKMS")) {
		t.Fatal("bad magic")
	}
	if len(blob) < 32+len(payload) {
		t.Fatal("blob too short")
	}
}
