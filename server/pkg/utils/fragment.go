package utils

import (
	"encoding/binary"
	"fmt"
)

// FragMagic returns build-seed derived fragment magic (aligned with Client wire_ids).
func FragMagic() []byte {
	m := GetWireIDs().FragMagic
	return m[:]
}

const fragHeaderSize = 9 // seq(4)+total(4)+flags(1)

// FragReassembler buffers CKF1 multi-frame messages per connection.
type FragReassembler struct {
	parts   [][]byte
	total   int
	filled  int
}

func NewFragReassembler() *FragReassembler {
	return &FragReassembler{}
}

func (r *FragReassembler) Reset() {
	r.parts = nil
	r.total = 0
	r.filled = 0
}

// OpenWire deobfuscates then either reassembles CKF1 fragments or decrypts a single frame.
// Returns (plaintext, needMore, error). When needMore is true, plaintext is nil.
func OpenWire(re *FragReassembler, wire []byte, obfMode string, sessionKey []byte) ([]byte, bool, error) {
	deobf := DeobfuscatePacket(wire, obfMode, sessionKey)

	if len(sessionKey) == 0 {
		return deobf, false, nil
	}

	fm := FragMagic()
	if len(deobf) >= 4 && deobf[0] == fm[0] && deobf[1] == fm[1] &&
		deobf[2] == fm[2] && deobf[3] == fm[3] {
		body := deobf[4:]
		if len(body) < fragHeaderSize {
			return nil, false, fmt.Errorf("short fragment")
		}
		seq := binary.BigEndian.Uint32(body[0:4])
		total := int(binary.BigEndian.Uint32(body[4:8]))
		if total <= 0 || total > 10000 {
			return nil, false, fmt.Errorf("invalid fragment total %d", total)
		}
		if re.parts == nil || re.total != total {
			re.parts = make([][]byte, total)
			re.total = total
			re.filled = 0
		}
		if int(seq) >= total {
			re.Reset()
			return nil, false, fmt.Errorf("fragment seq OOB %d/%d", seq, total)
		}
		if re.parts[seq] == nil {
			re.parts[seq] = append([]byte(nil), body...)
			re.filled++
		}
		if re.filled < total {
			return nil, true, nil
		}
		// Reassemble in order
		var plain []byte
		for i := 0; i < total; i++ {
			frag := re.parts[i]
			if frag == nil || len(frag) < fragHeaderSize {
				re.Reset()
				return nil, false, fmt.Errorf("missing fragment %d", i)
			}
			ct := frag[fragHeaderSize:]
			pt, err := DecryptAESWithCompat(ct, sessionKey)
			if err != nil {
				re.Reset()
				return nil, false, fmt.Errorf("fragment %d decrypt: %w", i, err)
			}
			plain = append(plain, pt...)
		}
		re.Reset()
		return plain, false, nil
	}

	// Legacy single-frame GCM
	if re.parts != nil {
		re.Reset()
	}
	pt, err := DecryptAESWithCompat(deobf, sessionKey)
	if err != nil {
		return nil, false, err
	}
	return pt, false, nil
}
