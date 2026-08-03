package hub

import (
	"testing"
)

func TestBroadcastDropsWhenFull(t *testing.T) {
	h := NewTaskHub()
	// 0-buffer channel: send never succeeds without a receiver
	ch := make(chan WsPacket)
	h.mu.Lock()
	h.subscribers["k"] = []chan WsPacket{ch}
	h.mu.Unlock()
	// no reader — every broadcast should eventually count as drop
	for i := 0; i < 5; i++ {
		h.Broadcast("k", WsPacket{Content: "x"})
	}
	if h.DroppedCount() == 0 {
		t.Fatal("expected drops when subscriber never receives")
	}
}
