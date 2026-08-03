package services

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestProductModuleWhitelist(t *testing.T) {
	if !IsProductModule("desktop") || !IsProductModule("iso_host") || !IsProductModule("inject") {
		t.Fatal("product modules must be allowed")
	}
	if IsProductModule("shell") || IsProductModule("bof") || IsProductModule("dotnet") {
		t.Fatal("legacy modules must not be product")
	}
}

func TestRegisterRejectsNonProduct(t *testing.T) {
	ms := NewModuleServiceForTest(t.TempDir())
	err := ms.RegisterRaw("shell", []byte{0x4d, 0x5a, 0x00})
	if !errors.Is(err, ErrModuleForbidden) {
		t.Fatalf("want ErrModuleForbidden, got %v", err)
	}
}

func TestPackCKMSRejectsLegacyDiskBlob(t *testing.T) {
	dir := t.TempDir()
	ms := NewModuleServiceForTest(dir)
	// Plant non-product bin on disk (simulates leftover bof.bin)
	if err := os.WriteFile(filepath.Join(dir, "bof.bin"), []byte{0x4d, 0x5a, 1, 2, 3}, 0o644); err != nil {
		t.Fatal(err)
	}
	_, err := ms.PackCKMS("bof")
	if !errors.Is(err, ErrModuleForbidden) {
		t.Fatalf("PackCKMS must refuse bof even if on disk: %v", err)
	}
	_, err = ms.PackCKMSWithKey("dotnet", DefaultModuleKey())
	if !errors.Is(err, ErrModuleForbidden) {
		t.Fatalf("PackCKMSWithKey must refuse dotnet: %v", err)
	}
}

func TestRegisterAndDeleteDesktopIsolated(t *testing.T) {
	ms := NewModuleServiceForTest(t.TempDir())
	pe := make([]byte, 64)
	pe[0], pe[1] = 'M', 'Z'
	id := "desktop"
	if err := ms.RegisterRaw(id, pe); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(ms.Dir(), id+".bin")
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("bin missing: %v", err)
	}
	found := false
	for _, e := range ms.ListCatalog("") {
		if e.ID == id {
			found = true
			break
		}
	}
	if !found {
		t.Fatal("desktop must appear in catalog after register")
	}
	if err := ms.Delete(id); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatal("bin should be removed")
	}
	err := ms.Delete(id)
	if !errors.Is(err, ErrModuleNotFound) {
		t.Fatalf("second delete want not found, got %v", err)
	}
	err = ms.Delete("shell")
	if !errors.Is(err, ErrModuleForbidden) {
		t.Fatalf("delete shell want forbidden, got %v", err)
	}
}

func TestRegisterDiskFailDoesNotPolluteMemory(t *testing.T) {
	// Point dir at a non-writable path by using a file as "dir"
	fileAsDir := filepath.Join(t.TempDir(), "notadir")
	if err := os.WriteFile(fileAsDir, []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	ms := NewModuleServiceForTest(fileAsDir) // MkdirAll on file fails later
	// Force dir to be the file path so WriteFile fails
	ms.dir = fileAsDir
	err := ms.RegisterRaw("inject", []byte{0x4d, 0x5a})
	if err == nil {
		t.Fatal("expected disk write failure")
	}
	if _, ok := ms.raw["inject"]; ok {
		t.Fatal("memory must not be updated when disk write fails")
	}
}

func TestCatalogNeverListsShellAsProduct(t *testing.T) {
	ms := NewModuleServiceForTest(t.TempDir())
	_ = os.WriteFile(filepath.Join(ms.Dir(), "shell.bin"), []byte{0x4d, 0x5a}, 0o644)
	// scanDisk-like: only product bins loaded into raw by Register path
	for _, e := range ms.ListCatalog("") {
		if e.ID == "shell" || e.ID == "bof" {
			t.Fatalf("non-product module %q must not appear in catalog", e.ID)
		}
	}
}
