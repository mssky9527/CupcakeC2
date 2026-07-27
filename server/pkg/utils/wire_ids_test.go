package utils

import (
	"testing"
)

func TestWireIDsStableDefault(t *testing.T) {
	a := DeriveWireIDs(DefaultWireSeed)
	b := DeriveWireIDs(DefaultWireSeed)
	if a.PkgMagic != b.PkgMagic || a.FragMagic != b.FragMagic || a.JobMagic != b.JobMagic {
		t.Fatal("not stable")
	}
	if len(a.NoiseInfo) != 16 || len(a.ModKeyDomain) != 16 {
		t.Fatal("domain len")
	}
	// Must not equal legacy ASCII brands
	if string(a.PkgMagic[:]) == "CKMS" {
		t.Fatal("still CKMS brand")
	}
	if string(a.FragMagic[:]) == "CKF1" {
		t.Fatal("still CKF1 brand")
	}
	if string(a.JobMagic[:]) == "CIS1" {
		t.Fatal("still CIS1 brand")
	}
}

func TestWireIDsDifferBySeed(t *testing.T) {
	a := DeriveWireIDs("seed-a")
	b := DeriveWireIDs("seed-b")
	if a.PkgMagic == b.PkgMagic {
		t.Fatal("different seeds must differ")
	}
}
