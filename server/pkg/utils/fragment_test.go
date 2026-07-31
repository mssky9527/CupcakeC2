package utils

import (
	"encoding/binary"
	"testing"
)

func TestOpenWireLegacySingle(t *testing.T) {
	key := make([]byte, 32)
	for i := range key {
		key[i] = byte(i + 1)
	}
	pt := []byte("hello-session-key")
	enc, err := EncryptAES(pt, key)
	if err != nil {
		t.Fatal(err)
	}
	re := NewFragReassembler()
	out, more, err := OpenWire(re, enc, "none", key)
	if err != nil || more {
		t.Fatalf("err=%v more=%v", err, more)
	}
	if string(out) != string(pt) {
		t.Fatalf("got %q", out)
	}
}

func TestOpenWireMissingFragmentDoesNotComplete(t *testing.T) {
	key := make([]byte, 32)
	for i := range key {
		key[i] = byte(i + 5)
	}
	chunks := [][]byte{[]byte("part0"), []byte("part1"), []byte("part2")}
	re := NewFragReassembler()
	// Only feed seq 0 and 2
	for _, seq := range []int{0, 2} {
		ct, err := EncryptAES(chunks[seq], key)
		if err != nil {
			t.Fatal(err)
		}
		body := make([]byte, 9+len(ct))
		binary.BigEndian.PutUint32(body[0:4], uint32(seq))
		binary.BigEndian.PutUint32(body[4:8], uint32(len(chunks)))
		body[8] = 0x01
		copy(body[9:], ct)
		wire := append(append([]byte{}, FragMagic()...), body...)
		out, more, err := OpenWire(re, wire, "none", key)
		if err != nil {
			t.Fatal(err)
		}
		if !more || out != nil {
			t.Fatalf("seq %d: expected needMore incomplete reassembly", seq)
		}
	}
}

func TestOpenWireCKF1Multi(t *testing.T) {
	key := make([]byte, 32)
	for i := range key {
		key[i] = byte(i + 3)
	}
	// Two fragments of plaintext chunks
	chunks := [][]byte{[]byte("AAAA"), []byte("BBBBCCCC")}
	re := NewFragReassembler()
	var complete []byte
	for seq, ch := range chunks {
		ct, err := EncryptAES(ch, key)
		if err != nil {
			t.Fatal(err)
		}
		body := make([]byte, 9+len(ct))
		binary.BigEndian.PutUint32(body[0:4], uint32(seq))
		binary.BigEndian.PutUint32(body[4:8], uint32(len(chunks)))
		if seq == len(chunks)-1 {
			body[8] = 0x02
		} else {
			body[8] = 0x01
		}
		copy(body[9:], ct)
		wire := append(append([]byte{}, FragMagic()...), body...)
		out, more, err := OpenWire(re, wire, "none", key)
		if err != nil {
			t.Fatal(err)
		}
		if seq < len(chunks)-1 {
			if !more || out != nil {
				t.Fatalf("expected needMore at seq %d", seq)
			}
		} else {
			if more {
				t.Fatal("expected complete")
			}
			complete = out
		}
	}
	want := append([]byte{}, chunks[0]...)
	want = append(want, chunks[1]...)
	if string(complete) != string(want) {
		t.Fatalf("got %q want %q", complete, want)
	}
}
