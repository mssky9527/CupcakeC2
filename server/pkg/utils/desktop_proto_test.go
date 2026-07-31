package utils

import (
	"bytes"
	"errors"
	"testing"
)

func TestEncodeParseDesktopHello(t *testing.T) {
	body := []byte(`{"fps":5,"encode":"jpeg"}`)
	msg, err := EncodeDesktopMessage(DesktopMsgHello, 0, body)
	if err != nil {
		t.Fatal(err)
	}
	env, err := ParseDesktopHeader(msg)
	if err != nil {
		t.Fatal(err)
	}
	if env.MsgType != DesktopMsgHello {
		t.Fatalf("type %d", env.MsgType)
	}
	if int(env.PayloadLen) != len(body) {
		t.Fatalf("len %d", env.PayloadLen)
	}
	if !bytes.Equal(msg[DesktopHeaderLen:], body) {
		t.Fatal("payload mismatch")
	}
}

func TestDesktopBadMagicSilent(t *testing.T) {
	msg, _ := EncodeDesktopMessage(DesktopMsgStop, 0, nil)
	msg[0] = 'X'
	_, err := ParseDesktopHeader(msg)
	if !errors.Is(err, ErrDesktopSilentClose) {
		t.Fatalf("want silent close got %v", err)
	}
}

func TestDesktopPayloadTooLarge(t *testing.T) {
	big := make([]byte, DesktopMaxPayload+1)
	_, err := EncodeDesktopMessage(DesktopMsgFrame, 0, big)
	if !errors.Is(err, ErrDesktopPayloadTooBig) {
		t.Fatalf("got %v", err)
	}
	hdr := make([]byte, 12)
	hdr[0], hdr[1], hdr[2], hdr[3] = DesktopMagic0, DesktopMagic1, DesktopMagic2, DesktopMagic3
	hdr[4] = DesktopProtoVersion
	hdr[5] = DesktopMsgFrame
	// payload_len = max+1
	hdr[8] = 1
	hdr[9] = 0
	hdr[10] = 0
	hdr[11] = 0x02 // little endian 0x02000001 huge — use proper:
	// Write max+1
	n := uint32(DesktopMaxPayload + 1)
	hdr[8] = byte(n)
	hdr[9] = byte(n >> 8)
	hdr[10] = byte(n >> 16)
	hdr[11] = byte(n >> 24)
	_, err = ParseDesktopHeader(hdr)
	if !errors.Is(err, ErrDesktopPayloadTooBig) {
		t.Fatalf("got %v", err)
	}
}

func TestMapInputPhysical(t *testing.T) {
	px, py, ok := MapInputToPhysical(640, 360, 1280, 720, 3840, 2160)
	if !ok || px != 1920 || py != 1080 {
		t.Fatalf("got %d,%d ok=%v", px, py, ok)
	}
}

func TestTokenBucketRateLimit(t *testing.T) {
	b := NewTokenBucket(1000, 0)
	if !b.Allow(800, 0) {
		t.Fatal("first allow")
	}
	if b.Allow(300, 0) {
		t.Fatal("should deny over capacity")
	}
	// refill 1s
	if !b.Allow(300, 1000) {
		t.Fatal("after refill")
	}
}
