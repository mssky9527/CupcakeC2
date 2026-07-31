#!/bin/bash

# Cupcake C2 - 服务端与前端全量编译脚本 (Linux / Bash)
# 作用: 自动编译前端 frontend-v2 并嵌入/输出到 server/dist，然后编译 Go 服务端可执行文件

set -e

# 颜色定义
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

# 确保脚本在根目录运行
cd "$(dirname "$0")"
PROJECT_ROOT=$(pwd)
SERVER_DIR="$PROJECT_ROOT/server"
FRONTEND_DIR="$SERVER_DIR/frontend-v2"
DIST_DIR="$SERVER_DIR/dist"
UI_DIR="$SERVER_DIR/ui"

echo -e "${BLUE}=========================================${NC}"
echo -e "${BLUE}   Cupcake C2 - 服务端与前端编译脚本     ${NC}"
echo -e "${BLUE}=========================================${NC}"
echo ""

# 1. 环境检查
echo -e "${YELLOW}[*] 正在检查编译环境...${NC}"

if ! command -v npm &> /dev/null; then
    echo -e "${RED}[!] 错误: 未检测到 Node.js/NPM，请先安装 Node.js。${NC}"
    exit 1
fi
echo -e "${GREEN}  [OK] Node.js/NPM 已就绪: $(npm --version)${NC}"

if ! command -v go &> /dev/null; then
    echo -e "${RED}[!] 错误: 未检测到 Go 环境，请先安装 Go (>= 1.20)。${NC}"
    exit 1
fi
echo -e "${GREEN}  [OK] Go 已就绪: $(go version)${NC}"
echo ""

# 2. 初始化目录
echo -e "${YELLOW}[*] 正在初始化存储与产物目录...${NC}"
mkdir -p "$SERVER_DIR/storage/payloads"
mkdir -p "$SERVER_DIR/storage/backups"
mkdir -p "$SERVER_DIR/assets"
mkdir -p "$DIST_DIR"

# 3. 编译前端
if [ -d "$FRONTEND_DIR" ]; then
    echo -e "${BLUE}[*] [1/2] 正在编译前端 Vue 应用 (frontend-v2)...${NC}"
    cd "$FRONTEND_DIR"
    
    if [ ! -d "node_modules" ]; then
        echo -e "${YELLOW}  [*] 正在安装前端依赖 (npm install)...${NC}"
        npm install
    fi
    
    echo -e "${YELLOW}  [*] 正在构建前端静态文件 (npm run build)...${NC}"
    npm run build
    
    cd "$PROJECT_ROOT"
    
    if [ -f "$DIST_DIR/index.html" ]; then
        echo -e "${GREEN}  [OK] 前端编译成功，产物存放在 server/dist${NC}"
        # 同步备份至 server/ui
        mkdir -p "$UI_DIR"
        cp -r "$DIST_DIR"/* "$UI_DIR/"
    else
        echo -e "${RED}  [!] 错误: 未能找到前端产物 index.html${NC}"
        exit 1
    fi
else
    echo -e "${RED}[!] 错误: 未找到前端源码目录 $FRONTEND_DIR${NC}"
    exit 1
fi
echo ""

# 4. 编译 Go 服务端
echo -e "${BLUE}[*] [2/2] 正在编译 Go 服务端 (Cupcake Server)..."
cd "$SERVER_DIR"

echo -e "${YELLOW}  [*] 正在整理 Go 依赖 (go mod tidy)...${NC}"
go mod tidy

echo -e "${YELLOW}  [*] 正在生成 Go 服务端二进制 (Hardened Build)...${NC}"
CGO_ENABLED=0 go build -v -ldflags="-s -w" -buildvcs=false -trimpath -o "cupcake-server" .

cd "$PROJECT_ROOT"

OUTPUT_BINARY="$SERVER_DIR/cupcake-server"
if [ -f "$OUTPUT_BINARY" ]; then
    chmod +x "$OUTPUT_BINARY"
    SIZE=$(du -h "$OUTPUT_BINARY" | cut -f1)
    echo ""
    echo -e "${BLUE}=========================================${NC}"
    echo -e "${GREEN}[DONE] 服务端与前端编译完成！${NC}"
    echo -e "${GREEN}[+] 服务端产物: $OUTPUT_BINARY ($SIZE)${NC}"
    echo -e "${GREEN}[+] 前端静态产物: $DIST_DIR${NC}"
    echo -e "${BLUE}=========================================${NC}"
    echo ""
else
    echo -e "${RED}[!] 编译产物丢失: $OUTPUT_BINARY${NC}"
    exit 1
fi
