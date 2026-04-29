package store

import (
    "cupcake-server/pkg/model"
)

// User store
func GetUserByUsername(username string) (*model.User, error) {
    var user model.User
    err := DB.Where("username = ?", username).First(&user).Error
    return &user, err
}

func GetAllUsers() ([]model.User, error) {
    var users []model.User
    err := DB.Find(&users).Error
    return users, err
}

func SaveUser(user *model.User) error {
    return DB.Save(user).Error
}

func DeleteUser(id uint) error {
    return DB.Delete(&model.User{}, id).Error
}

func SaveLoginLog(log *model.LoginLog) error {
    return DB.Create(log).Error
}

func GetLoginLogs(limit int) ([]model.LoginLog, error) {
    var logs []model.LoginLog
    err := DB.Order("created_at desc").Limit(limit).Find(&logs).Error
    return logs, err
}
