package services

import (
	"os"
	"path/filepath"
	"testing"
)

func TestRegisterAndPackShell(t *testing.T) {
	ms := GetModuleService()
	// go test cwd is services/; also try repo-relative paths
	candidates := []string{
		filepath.Join("..", "storage", "modules", "shell.bin"),
		filepath.Join("storage", "modules", "shell.bin"),
	}
	var loaded bool
	for _, p := range candidates {
		if err := ms.LoadFromFile("shell", p); err == nil {
			loaded = true
			break
		}
	}
	if !loaded {
		t.Skip("shell.bin missing (build cupcake-mod-shell and copy to storage/modules/shell.bin)")
	}
	b64, err := ms.PackBase64("shell")
	if err != nil {
		t.Fatal(err)
	}
	if len(b64) < 100 {
		t.Fatal("too short")
	}
	t.Logf("packed shell b64_len=%d", len(b64))
	_ = os.MkdirAll(filepath.Join("..", "storage", "modules"), 0o755)
	_ = os.WriteFile(filepath.Join("..", "storage", "modules", "shell.ckms.b64"), []byte(b64), 0644)
}
