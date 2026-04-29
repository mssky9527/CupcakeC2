package model

import (
    "time"
    "gorm.io/gorm"
)

type User struct {
    ID        uint           `gorm:"primaryKey" json:"id"`
    Username  string         `gorm:"uniqueIndex;size:100" json:"username"`
    Password  string         `json:"-"` // Never export hashed password
    Role      string         `gorm:"size:20;default:'operator'" json:"role"` // admin, operator
    Token     string         `gorm:"size:100;index" json:"-"` // Session token for web dashboard
    IsActive  bool           `gorm:"default:true" json:"is_active"`
    CreatedAt time.Time      `json:"created_at"`
    UpdatedAt time.Time      `json:"updated_at"`
    DeletedAt gorm.DeletedAt `gorm:"index" json:"-"`
}

type LoginLog struct {
    ID        uint      `gorm:"primaryKey" json:"id"`
    Username  string    `json:"username"`
    IP        string    `json:"ip"`
    UserAgent string    `json:"user_agent"`
    Status    string    `json:"status"` // success, failed
    Message   string    `json:"message"`
    CreatedAt time.Time `json:"created_at"`
}
