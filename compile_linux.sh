#!/bin/bash

# Cupcake C2 - Linux Agent 独立编译脚本
# 仅编译 Linux 版本的 Agent 模板，存放在 server/assets 中。

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
echo -e "${BLUE}    Cupcake C2 - Linux Template Compiler ${NC}"
echo -e "${BLUE}=========================================${NC}"

# 1. 检查 Rust 环境
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}[!] 未检测到 Cargo，请先安装 Rust 环境。${NC}"
    exit 1
fi

# 1.1 检测并安装 musl 工具链（Linux-musl 编译必须）
echo -e "${YELLOW}[*] 正在检查 musl 工具链...${NC}"
if ! command -v musl-gcc &> /dev/null && ! command -v x86_64-linux-musl-gcc &> /dev/null; then
    echo -e "${YELLOW}[*] 未检测到 musl-gcc，正在自动安装 musl-tools 和 musl-dev...${NC}"
    if [ -f /etc/debian_version ] || grep -qi 'ubuntu\|debian\|kali' /etc/os-release 2>/dev/null; then
        sudo apt-get update -y
        sudo apt-get install -y musl-tools musl-dev
        echo -e "${GREEN}[+] musl-tools 安装完成。${NC}"
    elif grep -qi 'centos\|fedora\|rhel' /etc/os-release 2>/dev/null; then
        sudo yum install -y musl musl-devel musl-gcc
        echo -e "${GREEN}[+] musl-tools 安装完成。${NC}"
    else
        echo -e "${RED}[!] 无法识别操作系统，请手动安装 musl-tools：sudo apt-get install musl-tools musl-dev${NC}"
        exit 1
    fi
else
    echo -e "${GREEN}[+] musl 工具链已就绪。${NC}"
fi

# 1.2 为 arm64 musl 安装交叉编译工具链（可选，仅在需要时安装）
if ! command -v aarch64-linux-musl-gcc &> /dev/null; then
    echo -e "${YELLOW}[*] 未检测到 aarch64 musl 交叉编译器，尝试安装...${NC}"
    if [ -f /etc/debian_version ] || grep -qi 'ubuntu\|debian\|kali' /etc/os-release 2>/dev/null; then
        sudo apt-get install -y gcc-aarch64-linux-gnu &>/dev/null || true
        echo -e "${GREEN}[+] arm64 交叉编译器（gnu）已就绪（将用作 arm64-musl 回退方案）。${NC}"
    fi
fi

# 2. 准备输出目录
mkdir -p "$ASSETS_DIR"

# 3. 编译函数
build_linux_template() {
    local arch=$1
    local proto=$2
    local output_name=$3
    local target=""

    echo -e "${YELLOW}[*] 正在构建 Linux 模板: $output_name (Arch: $arch, Feature: $proto)...${NC}"

    if [ "$arch" == "x64" ]; then
        target="x86_64-unknown-linux-musl"
    elif [ "$arch" == "arm64" ]; then
        # 注意: aarch64-linux-musl-gcc 在标准 apt 中不可用
        # 使用 aarch64-unknown-linux-gnu 替代，对应的 gcc-aarch64-linux-gnu 可直接安装
        target="aarch64-unknown-linux-gnu"
        export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
    fi

    # 尝试安装 target
    rustup target add "$target" >/dev/null 2>&1 || true

    cd "$CLIENT_DIR"
    
    # 🛡️ STEALTH: 移除本地路径前缀
    export RUSTFLAGS="--remap-path-prefix $CLIENT_DIR=/cupcake"

    # 执行编译 (使用 --no-default-features + capability profile)
    # UPX is intentionally NOT applied by default — high AV signature cost.
    if cargo build -p cupcake-core --release --target "$target" --no-default-features --features "$proto"; then
        local src_path="$CLIENT_DIR/target/$target/release/cupcake-core"
        if [ -f "$src_path" ]; then
            cp "$src_path" "$ASSETS_DIR/$output_name"
            chmod +x "$ASSETS_DIR/$output_name"
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

# 4. 执行批量编译任务
echo -e "${YELLOW}[*] 开始全量 Linux 模板编译进程...${NC}"

# --- x64 架构 (standard = product default post-ex) ---
build_linux_template "x64" "ws,standard"       "client_template_linux"
build_linux_template "x64" "tcp,standard"      "client_template_linux_tcp"
build_linux_template "x64" "dns,standard"      "client_template_linux_dns"
build_linux_template "x64" "tcp_bind,standard" "client_template_linux_bind"
build_linux_template "x64" "tcp,minimal"       "client_template_linux_tcp_minimal"

# --- ARM64 架构 ---
build_linux_template "arm64" "ws,standard"       "client_template_linux_arm64"
build_linux_template "arm64" "tcp_bind,standard" "client_template_linux_bind_arm64"

echo -e "${BLUE}-----------------------------------------${NC}"
echo -e "${GREEN}[DONE] 所有 Linux 模板已就绪。${NC}"
echo -e "${GREEN}[+] 产物目录: $ASSETS_DIR${NC}"
echo -e "${BLUE}-----------------------------------------${NC}"
