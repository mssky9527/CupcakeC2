package hub

import (
	"sync"
	"sync/atomic"
)

type WsPacket struct {
	MsgType string `json:"type"`    // "TERM" | "JSON_DATA" | "log" | "error" | "success"
	Content string `json:"content"` // The actual payload string
	TaskID  string `json:"task_id,omitempty"`
}

type TaskHub struct {
	subscribers map[string][]chan WsPacket
	history     map[string][]WsPacket
	mu          sync.Mutex
	// Dropped is readable for tests/metrics (packets not delivered to a subscriber).
	Dropped atomic.Uint64
}

func NewTaskHub() *TaskHub {
	return &TaskHub{
		subscribers: make(map[string][]chan WsPacket),
		history:     make(map[string][]WsPacket),
	}
}

// DroppedCount returns hub-level dropped packet count.
func (h *TaskHub) DroppedCount() uint64 {
	return h.Dropped.Load()
}

func (h *TaskHub) Broadcast(taskID string, packet WsPacket) {
	h.mu.Lock()
	defer h.mu.Unlock()

	// 1. Save to history (Limit to last 1000 lines to avoid memory bloat)
	if len(h.history[taskID]) < 1000 {
		h.history[taskID] = append(h.history[taskID], packet)
	}

	// 2. Broadcast to active listeners (drop + count when full — never block hub)
	if subs, ok := h.subscribers[taskID]; ok {
		for _, ch := range subs {
			select {
			case ch <- packet:
			default:
				// Best-effort: drop oldest then retry once
				select {
				case <-ch:
				default:
				}
				select {
				case ch <- packet:
				default:
					h.Dropped.Add(1)
				}
			}
		}
	}
}

func (h *TaskHub) Subscribe(taskID string) chan WsPacket {
	h.mu.Lock()
	defer h.mu.Unlock()
	ch := make(chan WsPacket, 1000) // Larger buffer

	// 1. Replay history to the new subscriber
	if hist, ok := h.history[taskID]; ok {
		for _, p := range hist {
			ch <- p
		}
	}

	h.subscribers[taskID] = append(h.subscribers[taskID], ch)
	return ch
}

func (h *TaskHub) Unsubscribe(taskID string, ch chan WsPacket) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if subs, ok := h.subscribers[taskID]; ok {
		for i, s := range subs {
			if s == ch {
				h.subscribers[taskID] = append(subs[:i], subs[i+1:]...)
				close(ch)
				// If no more subscribers, we COULD cleanup history, but build might still be running.
				// For now keep history until explicit cleanup or server restart.
				break
			}
		}
	}
}

func (h *TaskHub) Cleanup(taskID string) {
	h.mu.Lock()
	defer h.mu.Unlock()
	delete(h.history, taskID)
	// Optionally close all channels in subscribers[taskID] if needed
}

var BuildHub = NewTaskHub()
