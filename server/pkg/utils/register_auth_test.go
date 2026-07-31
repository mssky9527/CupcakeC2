package utils

import (
	"testing"
)

func TestRegisterProofAcceptsValidAndRejectsWrong(t *testing.T) {
	key := []byte("01234567890123456789012345678901")
	uuid := "11111111-2222-3333-4444-555555555555"

	proof := ComputeRegisterProof(key, uuid)
	if !VerifyRegisterProof(key, uuid, proof) {
		t.Fatal("valid proof rejected")
	}
	if VerifyRegisterProof(key, "other-uuid", proof) {
		t.Fatal("wrong uuid accepted")
	}
	if VerifyRegisterProof([]byte("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"), uuid, proof) {
		t.Fatal("wrong key accepted")
	}
	if VerifyRegisterProof(key, uuid, "") {
		t.Fatal("empty proof accepted")
	}
	if VerifyRegisterProof(key, uuid, "not-valid!!!") {
		t.Fatal("garbage proof accepted")
	}
	if VerifyRegisterProof(nil, uuid, proof) {
		t.Fatal("nil key accepted")
	}
	// Bare UUID alone is not a proof
	if VerifyRegisterProof(key, uuid, uuid) {
		t.Fatal("uuid-as-proof must fail")
	}
}

func TestRegisterProofDifferentKeysDiffer(t *testing.T) {
	a := ComputeRegisterProof([]byte("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), "u1")
	b := ComputeRegisterProof([]byte("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"), "u1")
	if a == b {
		t.Fatal("different keys produced identical proofs")
	}
}
