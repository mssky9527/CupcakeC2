// CPXD desktop session framing (pure codec).
// Mirrors Client/core/src/transport/desktop_proto.rs — keep values identical.
package utils

import (
	"encoding/binary"
	"errors"
)

const (
	DesktopMagic0 = 'C'
	DesktopMagic1 = 'P'
	DesktopMagic2 = 'X'
	DesktopMagic3 = 'D'

	DesktopProtoVersion = 1
	DesktopMaxPayload   = 2 * 1024 * 1024
	DesktopHeaderLen    = 12

	DesktopMsgHello      = 0x01
	DesktopMsgHelloAck   = 0x02
	DesktopMsgFrame      = 0x03
	DesktopMsgInput      = 0x04
	DesktopMsgConfig     = 0x05
	DesktopMsgPing       = 0x06
	DesktopMsgPong       = 0x07
	DesktopMsgError      = 0x08
	DesktopMsgStop       = 0x09
	DesktopMsgKeyframe   = 0x0A
	DesktopMsgStats      = 0x10

	DesktopEncodeJPEG = 1
	DesktopEncodeH264 = 2
)

var (
	ErrDesktopSilentClose   = errors.New("desktop: silent close (bad magic/version)")
	ErrDesktopTruncated     = errors.New("desktop: truncated header")
	ErrDesktopPayloadTooBig = errors.New("desktop: payload too large")
)

// DesktopEnvelope is the 12-byte CPXD header.
type DesktopEnvelope struct {
	Version    byte
	MsgType    byte
	Flags      uint16
	PayloadLen uint32
}

// ParseDesktopHeader parses a 12-byte header.
func ParseDesktopHeader(buf []byte) (DesktopEnvelope, error) {
	var e DesktopEnvelope
	if len(buf) < DesktopHeaderLen {
		return e, ErrDesktopTruncated
	}
	if buf[0] != DesktopMagic0 || buf[1] != DesktopMagic1 || buf[2] != DesktopMagic2 || buf[3] != DesktopMagic3 {
		return e, ErrDesktopSilentClose
	}
	if buf[4] != DesktopProtoVersion {
		return e, ErrDesktopSilentClose
	}
	e.Version = DesktopProtoVersion
	e.MsgType = buf[5]
	e.Flags = binary.LittleEndian.Uint16(buf[6:8])
	e.PayloadLen = binary.LittleEndian.Uint32(buf[8:12])
	if e.PayloadLen > DesktopMaxPayload {
		return e, ErrDesktopPayloadTooBig
	}
	return e, nil
}

// EncodeDesktopMessage builds magic|ver|type|flags|len|payload.
func EncodeDesktopMessage(msgType byte, flags uint16, payload []byte) ([]byte, error) {
	if len(payload) > DesktopMaxPayload {
		return nil, ErrDesktopPayloadTooBig
	}
	out := make([]byte, DesktopHeaderLen+len(payload))
	out[0], out[1], out[2], out[3] = DesktopMagic0, DesktopMagic1, DesktopMagic2, DesktopMagic3
	out[4] = DesktopProtoVersion
	out[5] = msgType
	binary.LittleEndian.PutUint16(out[6:8], flags)
	binary.LittleEndian.PutUint32(out[8:12], uint32(len(payload)))
	copy(out[DesktopHeaderLen:], payload)
	return out, nil
}

// MapInputToPhysical maps frame-space coords to physical pixels.
func MapInputToPhysical(x, y, frameW, frameH, physW, physH uint16) (uint16, uint16, bool) {
	if frameW == 0 || frameH == 0 || physW == 0 || physH == 0 {
		return 0, 0, false
	}
	px := uint16(uint32(x) * uint32(physW) / uint32(frameW))
	py := uint16(uint32(y) * uint32(physH) / uint32(frameH))
	if px >= physW {
		px = physW - 1
	}
	if py >= physH {
		py = physH - 1
	}
	return px, py, true
}

// TokenBucket is a simple per-stream byte rate limiter (pure helper).
type TokenBucket struct {
	Capacity   int64
	Tokens     int64
	RefillPerS int64
	LastUnixMs int64
}

// NewTokenBucket creates a bucket with maxBytesPerSec refill.
func NewTokenBucket(maxBytesPerS int64, nowMs int64) *TokenBucket {
	if maxBytesPerS <= 0 {
		maxBytesPerS = 2_000_000
	}
	return &TokenBucket{
		Capacity:   maxBytesPerS,
		Tokens:     maxBytesPerS,
		RefillPerS: maxBytesPerS,
		LastUnixMs: nowMs,
	}
}

// Allow returns true if n bytes may pass; deducts tokens on success.
func (b *TokenBucket) Allow(n int64, nowMs int64) bool {
	if b == nil {
		return true
	}
	if nowMs > b.LastUnixMs {
		elapsed := nowMs - b.LastUnixMs
		add := b.RefillPerS * elapsed / 1000
		b.Tokens += add
		if b.Tokens > b.Capacity {
			b.Tokens = b.Capacity
		}
		b.LastUnixMs = nowMs
	}
	if n <= 0 {
		return true
	}
	if b.Tokens < n {
		return false
	}
	b.Tokens -= n
	return true
}

// DesktopRateDefaults matches design §5.1.
const (
	DesktopDefaultMaxBps        = 2_000_000
	DesktopDefaultMaxFps        = 5
	DesktopDefaultMaxFrameBytes = 512_000
)
