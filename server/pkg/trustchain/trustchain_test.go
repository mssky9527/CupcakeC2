package trustchain

import (
	"crypto/sha256"
	"encoding/hex"
	"strings"
	"testing"
)

func testPayloadSHA(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func TestSignThenVerifyOK(t *testing.T) {
	key := []byte("unit-test-hmac-key-32bytes!!!!!!")
	meta := PackageMeta{
		ModuleID:   "plugin-a",
		Version:    "1.2.3",
		SHA256:     testPayloadSHA([]byte("payload")),
		Target:     "windows-x64",
		ABIVersion: 1,
		Signer:     "test-key-1",
	}
	sig, err := Sign(meta, key)
	if err != nil {
		t.Fatalf("Sign: %v", err)
	}
	if len(sig) != 64 {
		t.Fatalf("sig hex len %d", len(sig))
	}
	meta.Signature = sig
	if err := Verify(meta, key); err != nil {
		t.Fatalf("Verify: %v", err)
	}
}

func TestWrongSignatureFails(t *testing.T) {
	key := []byte("unit-test-hmac-key-32bytes!!!!!!")
	meta := PackageMeta{
		ModuleID: "m1",
		Version:  "1.0.0",
		SHA256:   testPayloadSHA([]byte("x")),
		Signer:   "k",
	}
	sig, err := Sign(meta, key)
	if err != nil {
		t.Fatal(err)
	}
	meta.Signature = sig
	// flip one nibble
	b := []byte(meta.Signature)
	if b[0] == 'a' {
		b[0] = 'b'
	} else {
		b[0] = 'a'
	}
	meta.Signature = string(b)
	verr := Verify(meta, key)
	if verr == nil {
		t.Fatal("wrong signature must fail")
	}
	if !strings.Contains(verr.Error(), "signature mismatch") {
		t.Fatalf("unexpected err: %v", verr)
	}
}

func TestEmptySignatureFails(t *testing.T) {
	key := []byte("k")
	meta := PackageMeta{
		ModuleID:  "m",
		Version:   "1",
		SHA256:    "abcd",
		Signer:    "s",
		Signature: "",
	}
	err := Verify(meta, key)
	if err == nil {
		t.Fatal("empty signature must fail")
	}
	if !strings.Contains(err.Error(), "empty signature") {
		t.Fatalf("unexpected: %v", err)
	}
}

func TestTamperedHashFailsVerify(t *testing.T) {
	key := []byte("unit-test-hmac-key-32bytes!!!!!!")
	meta := PackageMeta{
		ModuleID:   "mod",
		Version:    "2.0.0",
		SHA256:     testPayloadSHA([]byte("good")),
		ABIVersion: 2,
		Signer:     "test-key-1",
	}
	sig, err := Sign(meta, key)
	if err != nil {
		t.Fatal(err)
	}
	meta.Signature = sig
	meta.SHA256 = testPayloadSHA([]byte("tampered"))
	if err := Verify(meta, key); err == nil {
		t.Fatal("tampered sha256 must fail verify")
	}
}

func TestRollbackGuard(t *testing.T) {
	g := NewRollbackGuard()
	if err := g.CheckAndCommit("p1", "1.2.0"); err != nil {
		t.Fatalf("commit 1.2.0: %v", err)
	}
	err := g.CheckAndCommit("p1", "1.1.0")
	if err == nil {
		t.Fatal("rollback 1.1.0 must fail")
	}
	if !strings.Contains(err.Error(), "version rollback refused") {
		t.Fatalf("unexpected: %v", err)
	}
	// equal re-publish ok
	if err := g.CheckAndCommit("p1", "1.2.0"); err != nil {
		t.Fatalf("equal version should allow: %v", err)
	}
	if err := g.CheckAndCommit("p1", "1.2.1"); err != nil {
		t.Fatalf("higher 1.2.1 should ok: %v", err)
	}
	if g.MaxVersion("p1") != "1.2.1" {
		t.Fatalf("max=%s", g.MaxVersion("p1"))
	}
}

func TestMissingKeyVerifyFails(t *testing.T) {
	meta := PackageMeta{
		ModuleID:  "m",
		Version:   "1.0.0",
		SHA256:    "aa",
		Signer:    "s",
		Signature: "00",
	}
	if err := Verify(meta, nil); err == nil {
		t.Fatal("nil key must fail")
	}
	if err := Verify(meta, []byte{}); err == nil {
		t.Fatal("empty key must fail")
	}
	if _, err := Sign(meta, nil); err == nil {
		t.Fatal("Sign with nil key must fail")
	}
}

func TestCompareVersion(t *testing.T) {
	cases := []struct {
		a, b string
		want int
	}{
		{"1.2.0", "1.1.0", 1},
		{"1.1.0", "1.2.0", -1},
		{"1.2.0", "1.2.0", 0},
		{"3", "2.9.9", 1},
		{"1.0", "1.0.0", 0},
		{"2.0.0", "2", 0},
	}
	for _, tc := range cases {
		got := CompareVersion(tc.a, tc.b)
		if got != tc.want {
			t.Errorf("CompareVersion(%q,%q)=%d want %d", tc.a, tc.b, got, tc.want)
		}
	}
}

func TestCanonicalPayloadEmptyTarget(t *testing.T) {
	meta := PackageMeta{
		ModuleID:   "id",
		Version:    "1",
		SHA256:     "abc",
		Target:     "",
		ABIVersion: 0,
		Signer:     "s",
	}
	got := CanonicalPayload(meta)
	want := "id|1|abc||0|s"
	if got != want {
		t.Fatalf("got %q want %q", got, want)
	}
}

func TestHMACKeyForSignerDevKeys(t *testing.T) {
	t.Setenv("CUPCAKE_TRUST_HMAC_KEY", "")
	t.Setenv("CUPCAKE_TRUST_DEV_KEYS", "1")
	k1 := HMACKeyForSigner("test-key-1")
	if len(k1) != 32 {
		t.Fatalf("dev key len %d", len(k1))
	}
	k2 := HMACKeyForSigner("other")
	if hex.EncodeToString(k1) == hex.EncodeToString(k2) {
		t.Fatal("different signers should yield different dev keys")
	}
	// with explicit env key, that wins
	t.Setenv("CUPCAKE_TRUST_HMAC_KEY", "from-env")
	kEnv := HMACKeyForSigner("anything")
	if string(kEnv) != "from-env" {
		t.Fatalf("env key should win: %q", kEnv)
	}
}

func TestHMACKeyForSignerProductionNoKey(t *testing.T) {
	t.Setenv("CUPCAKE_TRUST_HMAC_KEY", "")
	t.Setenv("CUPCAKE_TRUST_DEV_KEYS", "")
	if k := HMACKeyForSigner("x"); k != nil {
		t.Fatalf("expected nil key, got %v", k)
	}
}
