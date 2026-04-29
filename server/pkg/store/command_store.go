package store

import (
	"cupcake-server/pkg/model"
	"fmt"
	"os"
	"path/filepath"
	"time"
)

func CreateCommandLog(agentUUID, reqID, cmdType, input string) error {
	log := model.CommandLog{
		AgentUUID: agentUUID,
		ReqID:     reqID,
		Type:      cmdType,
		Input:     input,
		Status:    "pending",
		CreatedAt: time.Now(),
		UpdatedAt: time.Now(),
	}
	return DB.Create(&log).Error
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
	
	// [NEW] Persist to physical log file for independent viewing
	logPath := filepath.Join("storage/logs", fmt.Sprintf("task_%s.txt", reqID))
	os.MkdirAll("storage/logs", 0755)
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
