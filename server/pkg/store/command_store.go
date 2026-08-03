package store

import (
	"fmt"
	"log"
	"os"
	"path/filepath"
	"strings"
	"time"

	"cupcake-server/pkg/model"
	"cupcake-server/pkg/paths"
)

// TaskLogRetentionDays is how long task_*.txt and matching DB rows are kept.
// Override with env CUPCAKE_TASK_LOG_RETENTION_DAYS (integer days, min 1).
const TaskLogRetentionDaysDefault = 7

func CreateCommandLog(agentUUID, reqID, cmdType, input string) error {
	logEntry := model.CommandLog{
		AgentUUID: agentUUID,
		ReqID:     reqID,
		Type:      cmdType,
		Input:     input,
		Status:    "pending",
		CreatedAt: time.Now(),
		UpdatedAt: time.Now(),
	}
	return DB.Create(&logEntry).Error
}

func UpdateCommandOutput(reqID, stdout, stderr string) error {
	output := stdout
	if stderr != "" {
		if output != "" {
			output += "\n[STDERR]\n" + stderr
		} else {
			output = "[STDERR] " + stderr
		}
	}

	// Persist to physical log file for independent viewing
	logDir := paths.Join("logs")
	logPath := filepath.Join(logDir, fmt.Sprintf("task_%s.txt", reqID))
	os.MkdirAll(logDir, 0755)
	_ = os.WriteFile(logPath, []byte(output), 0644)

	return DB.Model(&model.CommandLog{}).Where("req_id = ?", reqID).Updates(map[string]interface{}{
		"output":     output,
		"status":     "completed",
		"updated_at": time.Now(),
	}).Error
}

func GetCommandHistory(agentUUID string) ([]model.CommandLog, error) {
	var logs []model.CommandLog
	err := DB.Where("agent_uuid = ?", agentUUID).Order("created_at desc").Find(&logs).Error
	return logs, err
}

// TaskLogRetentionDays resolves retention period from env or default.
func TaskLogRetentionDays() int {
	v := strings.TrimSpace(os.Getenv("CUPCAKE_TASK_LOG_RETENTION_DAYS"))
	if v == "" {
		return TaskLogRetentionDaysDefault
	}
	var n int
	if _, err := fmt.Sscanf(v, "%d", &n); err != nil || n < 1 {
		return TaskLogRetentionDaysDefault
	}
	return n
}

// PurgeExpiredTaskLogs deletes task log files and DB rows older than retention.
// Returns counts for tests/diagnostics.
func PurgeExpiredTaskLogs(olderThan time.Duration) (filesRemoved int, rowsRemoved int64, err error) {
	if olderThan <= 0 {
		olderThan = time.Duration(TaskLogRetentionDays()) * 24 * time.Hour
	}
	cutoff := time.Now().Add(-olderThan)
	logDir := paths.Join("logs")

	entries, readErr := os.ReadDir(logDir)
	if readErr != nil && !os.IsNotExist(readErr) {
		return 0, 0, readErr
	}
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		name := e.Name()
		if !strings.HasPrefix(name, "task_") || !strings.HasSuffix(name, ".txt") {
			continue
		}
		full := filepath.Join(logDir, name)
		info, statErr := e.Info()
		if statErr != nil {
			continue
		}
		if info.ModTime().Before(cutoff) {
			if rmErr := os.Remove(full); rmErr == nil {
				filesRemoved++
			}
		}
	}

	if DB != nil {
		res := DB.Where("updated_at < ? OR (updated_at IS NULL AND created_at < ?)", cutoff, cutoff).
			Delete(&model.CommandLog{})
		if res.Error != nil {
			return filesRemoved, 0, res.Error
		}
		rowsRemoved = res.RowsAffected
	}
	return filesRemoved, rowsRemoved, nil
}

// StartTaskLogRetentionWorker runs periodic purge of old task logs.
// interval defaults to 1 hour when zero.
func StartTaskLogRetentionWorker(interval time.Duration) {
	if interval <= 0 {
		interval = time.Hour
	}
	go func() {
		// Initial delay so startup is not blocked on large purges
		time.Sleep(30 * time.Second)
		for {
			days := TaskLogRetentionDays()
			files, rows, err := PurgeExpiredTaskLogs(time.Duration(days) * 24 * time.Hour)
			if err != nil {
				log.Printf("[retention] task log purge error: %v", err)
			} else if files > 0 || rows > 0 {
				log.Printf("[retention] purged task logs files=%d db_rows=%d retention_days=%d", files, rows, days)
			}
			time.Sleep(interval)
		}
	}()
}
