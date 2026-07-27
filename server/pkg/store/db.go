package store

import (
	"crypto/rand"
	"log"
	"math/big"
	"os"
	"path/filepath"

	"cupcake-server/pkg/model"
	"cupcake-server/pkg/paths"

	"github.com/glebarez/sqlite"
	"golang.org/x/crypto/bcrypt"
	"gorm.io/gorm"
)

var DB *gorm.DB

func InitDB() {
	var err error
	paths.Init()
	dbPath := paths.Join("cupcake.db")
	
	// Create storage directory if not exists
	if err := os.MkdirAll(filepath.Dir(dbPath), 0755); err != nil {
		log.Fatalf("Failed to create storage directory: %v", err)
	}

	DB, err = gorm.Open(sqlite.Open(dbPath), &gorm.Config{})
	if err != nil {
		log.Fatalf("Failed to connect to database: %v", err)
	}

	// Auto Migrate
	err = DB.AutoMigrate(
		&model.Agent{}, 
		&model.CommandLog{}, 
		&model.Listener{},
		&model.User{},
		&model.LoginLog{},
		&model.GlobalSetting{},
		&model.NotificationWebhook{},
		&model.Tunnel{},
	)
	if err != nil {
		log.Fatalf("Failed to migrate database: %v", err)
	}

	// Initialize default admin if no users exist
	initDefaultAdmin()
}

func initDefaultAdmin() {
	var count int64
	DB.Model(&model.User{}).Count(&count)
	if count == 0 {
		hashed, _ := HashPassword("cupcake123")
		admin := model.User{
			Username: "admin",
			Password: hashed,
			Role:     "admin",
		}
		DB.Create(&admin)
	}

	// Initialize API Token
	var tokenCount int64
	DB.Model(&model.GlobalSetting{}).Where("key = ?", "system_api_token").Count(&tokenCount)
	if tokenCount == 0 {
		token := GenerateSecureToken(32)
		DB.Create(&model.GlobalSetting{
			Key:   "system_api_token",
			Value: token,
			Group: "security",
		})
	}

	// Initialize MCP Status
	var mcpCount int64
	DB.Model(&model.GlobalSetting{}).Where("key = ?", "system_mcp_enabled").Count(&mcpCount)
	if mcpCount == 0 {
		DB.Create(&model.GlobalSetting{
			Key:   "system_mcp_enabled",
			Value: "true",
			Group: "security",
		})
	}
}

func HashPassword(password string) (string, error) {
	bytes, err := bcrypt.GenerateFromPassword([]byte(password), 14)
	return string(bytes), err
}

func CheckPasswordHash(password, hash string) bool {
	err := bcrypt.CompareHashAndPassword([]byte(hash), []byte(password))
	return err == nil
}

func GenerateSecureToken(length int) string {
	const charset = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.~"
	result := make([]byte, length)
	for i := range result {
		n, _ := rand.Int(rand.Reader, big.NewInt(int64(len(charset))))
		result[i] = charset[n.Int64()]
	}
	return string(result)
}

func isHexString(s string) bool {
	for _, c := range s {
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')) {
			return false
		}
	}
	return true
}
