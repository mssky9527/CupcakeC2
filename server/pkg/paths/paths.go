package paths

import (
	"os"
	"path/filepath"
	"sync"
)

var (
	mu   sync.RWMutex
	root string
)

// Init sets storage root from CUPCAKE_DATA_DIR or default "storage".
func Init() {
	mu.Lock()
	defer mu.Unlock()
	if v := os.Getenv("CUPCAKE_DATA_DIR"); v != "" {
		root = v
	} else {
		root = "storage"
	}
	_ = os.MkdirAll(root, 0755)
}

// Root returns configured data directory.
func Root() string {
	mu.RLock()
	defer mu.RUnlock()
	if root == "" {
		return "storage"
	}
	return root
}

// Join joins path under data root.
func Join(elem ...string) string {
	parts := append([]string{Root()}, elem...)
	return filepath.Join(parts...)
}
