#!/bin/bash

# Cupcake C2 - Windows Agent 交叉编译脚本 (Linux -> Windows)
# 使用 mingw-w64 编译器在 Linux 上生成 Windows 版本的 Agent 模板。

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
CLIENT_DIR="$PROJECT_ROOT/Client"
ASSETS_DIR="$PROJECT_ROOT/server/assets"

echo -e "${BLUE}=========================================${NC}"
echo -e "${BLUE}   Cupcake C2 - Windows Cross-Compiler   ${NC}"
echo -e "${BLUE}=========================================${NC}"

# 1. 检查 Rust 环境
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}[!] 未检测到 Cargo，请先安装 Rust 环境。${NC}"
    exit 1
fi

# 2. 检查 MinGW 环境
if ! command -v x86_64-w64-mingw32-gcc &> /dev/null; then
    echo -e "${YELLOW}[*] 未检测到 x86_64-w64-mingw32-gcc，正在尝试安装...${NC}"
    sudo apt-get update && sudo apt-get install -y gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64
fi

# 3. 准备输出目录
mkdir -p "$ASSETS_DIR"

# 4. 编译函数
build_windows_template() {
    local arch=$1
    local proto=$2
    local output_name=$3
    local target=""

    echo -e "${YELLOW}[*] 正在构建 Windows 模板: $output_name (Arch: $arch, Feature: $proto)...${NC}"

    if [ "$arch" == "x64" ]; then
        target="x86_64-pc-windows-gnu"
    elif [ "$arch" == "x86" ]; then
        target="i686-pc-windows-gnu"
        if ! command -v i686-w64-mingw32-gcc &> /dev/null; then
             sudo apt-get install -y gcc-mingw-w64-i686 g++-mingw-w64-i686
        fi
    fi

    # 尝试安装 target
    rustup target add "$target" >/dev/null 2>&1 || true

    cd "$CLIENT_DIR"
    
    # 🛡️ STEALTH: 移除本地路径前缀
    export RUSTFLAGS="--remap-path-prefix $CLIENT_DIR=/cupcake"

    # 执行编译
    if cargo build -p cupcake-core --release --target "$target" --no-default-features --features "$proto"; then
        local src_path="$CLIENT_DIR/target/$target/release/cupcake-core.exe"
        if [ -f "$src_path" ]; then
            cp "$src_path" "$ASSETS_DIR/$output_name"
            echo -e "${GREEN}[+] 成功生成: $output_name${NC}"
        else
            echo -e "${RED}[!] 错误: 产物文件丢失${NC}"
            exit 1
        fi
    else
        echo -e "${RED}[!] 编译失败: $output_name${NC}"
        exit 1
    fi
    cd ..
}

# 5. 执行批量编译任务
echo -e "${YELLOW}[*] 开始全量 Windows 模板编译进程 (GNU 链)...${NC}"

# WebSocket
build_windows_template "x64" "ws"       "client_template_windows.exe"
build_windows_template "x86" "ws"       "client_template_windows_x86.exe"

# Reverse TCP
build_windows_template "x64" "tcp"      "client_template_windows_tcp.exe"

# Bind TCP
build_windows_template "x64" "tcp_bind" "client_template_windows_bind.exe"

# DNS
build_windows_template "x64" "dns"      "client_template_windows_dns.exe"

echo -e "${BLUE}-----------------------------------------${NC}"
echo -e "${GREEN}[DONE] Windows 模板编译完成。${NC}"
echo -e "${BLUE}-----------------------------------------${NC}"
