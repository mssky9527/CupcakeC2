package services

import "testing"

func TestDetectFscanLikeNative(t *testing.T) {
	pe := make([]byte, 0x200)
	pe[0], pe[1] = 'M', 'Z'
	pe[0x3c] = 0x80
	copy(pe[0x80:], []byte("PE\x00\x00"))
	pe[0x80+24] = 0x0b
	pe[0x80+25] = 0x20
	got := DetectPluginExecType(pe, "fscan.exe")
	if got != "native-exec" {
		t.Fatalf("got %s want native-exec", got)
	}
}

func TestDetectBofCoff(t *testing.T) {
	b := make([]byte, 40)
	b[0], b[1] = 0x64, 0x86
	b[2], b[3] = 3, 0
	got := DetectPluginExecType(b, "whoami.o")
	if got != "bof-exec" {
		t.Fatalf("got %s want bof-exec", got)
	}
}
