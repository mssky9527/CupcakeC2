package store

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"cupcake-server/pkg/model"
	"cupcake-server/pkg/paths"
)

func TestTaskLogRetentionDaysDefault(t *testing.T) {
	t.Setenv("CUPCAKE_TASK_LOG_RETENTION_DAYS", "")
	if d := TaskLogRetentionDays(); d != TaskLogRetentionDaysDefault {
		t.Fatalf("got %d want %d", d, TaskLogRetentionDaysDefault)
	}
}

func TestTaskLogRetentionDaysEnv(t *testing.T) {
	t.Setenv("CUPCAKE_TASK_LOG_RETENTION_DAYS", "3")
	if d := TaskLogRetentionDays(); d != 3 {
		t.Fatalf("got %d want 3", d)
	}
	t.Setenv("CUPCAKE_TASK_LOG_RETENTION_DAYS", "0")
	if d := TaskLogRetentionDays(); d != TaskLogRetentionDaysDefault {
		t.Fatalf("invalid 0 should fall back, got %d", d)
	}
}

func TestPurgeExpiredTaskLogs(t *testing.T) {
	tmp := t.TempDir()
	t.Setenv("CUPCAKE_DATA_DIR", tmp)
	paths.Init()

	// Fresh sqlite under tmp via production InitDB
	dbPath := paths.Join("cupcake.db")
	_ = os.MkdirAll(filepath.Dir(dbPath), 0755)
	InitDB()
	if DB == nil {
		t.Fatal("DB nil")
	}
	// Release file lock so TempDir cleanup succeeds on Windows.
	t.Cleanup(func() {
		if DB != nil {
			if sqlDB, err := DB.DB(); err == nil {
				_ = sqlDB.Close()
			}
			DB = nil
		}
	})

	logDir := paths.Join("logs")
	_ = os.MkdirAll(logDir, 0755)

	oldFile := filepath.Join(logDir, "task_oldreq.txt")
	newFile := filepath.Join(logDir, "task_newreq.txt")
	if err := os.WriteFile(oldFile, []byte("old"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(newFile, []byte("new"), 0644); err != nil {
		t.Fatal(err)
	}
	// Age the old file
	oldTime := time.Now().Add(-10 * 24 * time.Hour)
	if err := os.Chtimes(oldFile, oldTime, oldTime); err != nil {
		t.Fatal(err)
	}

	// DB rows
	oldLog := model.CommandLog{
		AgentUUID: "a",
		ReqID:     "oldreq",
		Type:      "shell",
		Status:    "completed",
		CreatedAt: oldTime,
		UpdatedAt: oldTime,
	}
	newLog := model.CommandLog{
		AgentUUID: "a",
		ReqID:     "newreq",
		Type:      "shell",
		Status:    "completed",
		CreatedAt: time.Now(),
		UpdatedAt: time.Now(),
	}
	if err := DB.Create(&oldLog).Error; err != nil {
		t.Fatal(err)
	}
	if err := DB.Create(&newLog).Error; err != nil {
		t.Fatal(err)
	}

	files, rows, err := PurgeExpiredTaskLogs(7 * 24 * time.Hour)
	if err != nil {
		t.Fatal(err)
	}
	if files < 1 {
		t.Fatalf("expected at least 1 file removed, got %d", files)
	}
	if rows < 1 {
		t.Fatalf("expected at least 1 row removed, got %d", rows)
	}
	if _, err := os.Stat(oldFile); !os.IsNotExist(err) {
		t.Fatal("old file should be gone")
	}
	if _, err := os.Stat(newFile); err != nil {
		t.Fatal("new file should remain")
	}
	var count int64
	DB.Model(&model.CommandLog{}).Where("req_id = ?", "newreq").Count(&count)
	if count != 1 {
		t.Fatalf("new row should remain, count=%d", count)
	}
	DB.Model(&model.CommandLog{}).Where("req_id = ?", "oldreq").Count(&count)
	if count != 0 {
		t.Fatalf("old row should be gone, count=%d", count)
	}
}
