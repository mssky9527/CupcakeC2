package logx

import (
	"log/slog"
	"os"
	"sync"
)

var (
	once   sync.Once
	logger *slog.Logger
)

// L returns the process structured logger (JSON to stderr).
func L() *slog.Logger {
	once.Do(func() {
		handler := slog.NewJSONHandler(os.Stderr, &slog.HandlerOptions{Level: slog.LevelInfo})
		logger = slog.New(handler)
	})
	return logger
}

func Info(msg string, args ...any)  { L().Info(msg, args...) }
func Warn(msg string, args ...any)  { L().Warn(msg, args...) }
func Error(msg string, args ...any) { L().Error(msg, args...) }
func Debug(msg string, args ...any) { L().Debug(msg, args...) }
