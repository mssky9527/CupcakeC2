#!/bin/bash

# Cupcake C2 - Linux 一键启动脚本
# 用于在 Linux 环境下快速部署管理面板与服务端

set -e

# 确保脚本在自身所在目录下运行
cd "$(dirname "$0")"
SCRIPT_DIR=$(pwd)

# 颜色定义
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BLUE}=========================================${NC}"
echo -e "${BLUE}    Cupcake C2 - 服务端启动脚本          ${NC}"
echo -e "${BLUE}=========================================${NC}"

# 1. 环境全自动安装
echo -e "${YELLOW}[*] 正在检查并配置开发环境 (Go, Rust, Cross-Compile)...${NC}"

# 检测操作系统
if [ -f /etc/os-release ]; then
    . /etc/os-release
    OS=$ID
else
    OS=$(uname -s)
fi

# --- 补回安装 Go 逻辑 ---
# 使用 || true 防止 set -e 在没检测到命令时退出
GO_PATH_CHECK=$(command -v go || which go || ls /usr/local/go/bin/go 2>/dev/null || echo "")
if [ -z "$GO_PATH_CHECK" ]; then
    echo -e "${YELLOW}[*] 未检测到 Go，正在尝试安装 Go 1.25.4 (使用国内镜像)...${NC}"
    wget https://golang.google.cn/dl/go1.25.4.linux-amd64.tar.gz
    sudo rm -rf /usr/local/go && sudo tar -C /usr/local -xzf go1.25.4.linux-amd64.tar.gz
    rm go1.25.4.linux-amd64.tar.gz
    export PATH=$PATH:/usr/local/go/bin
    if ! grep -q "/usr/local/go/bin" /etc/profile; then
        echo 'export PATH=$PATH:/usr/local/go/bin' | sudo tee -a /etc/profile
    fi
    echo 'export PATH=$PATH:/usr/local/go/bin' >> ~/.bashrc
else
    if [[ ! "$PATH" == *"/usr/local/go/bin"* ]]; then
        export PATH=$PATH:/usr/local/go/bin
    fi
    echo -e "${GREEN}[+ ] Go 已就绪: $(go version)${NC}"
fi

# --- 补回安装 Rust 逻辑 ---
RUST_C_CHECK=$(command -v rustc || ls $HOME/.cargo/bin/rustc 2>/dev/null || ls /root/.cargo/bin/rustc 2>/dev/null || echo "")
if [ -z "$RUST_C_CHECK" ]; then
    echo -e "${YELLOW}[*] 未检测到 Rust，正在尝试自动安装 Rust (使用中科大镜像加速)...${NC}"
    export RUSTUP_DIST_SERVER=https://mirrors.ustc.edu.cn/rust-static
    export RUSTUP_UPDATE_ROOT=https://mirrors.ustc.edu.cn/rust-static/rustup
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    source $HOME/.cargo/env
    echo 'export RUSTUP_DIST_SERVER=https://mirrors.ustc.edu.cn/rust-static' >> ~/.bashrc
    echo 'export RUSTUP_UPDATE_ROOT=https://mirrors.ustc.edu.cn/rust-static/rustup' >> ~/.bashrc
    echo 'source $HOME/.cargo/env' >> ~/.bashrc
else
    if [[ ! "$PATH" == *"$HOME/.cargo/bin"* ]]; then
        export PATH="$HOME/.cargo/bin:$PATH"
        [ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
    fi
    echo -e "${GREEN}[+ ] Rust 已就绪: $(rustc --version)${NC}"
fi

install_dependencies() {
    # 检查关键工具是否已安装 (以 mingw 和 build-essential 的编译器为代表)
    NEED_INSTALL=false
    if ! command -v x86_64-w64-mingw32-gcc &> /dev/null || ! command -v gcc &> /dev/null || ! command -v make &> /dev/null; then
        NEED_INSTALL=true
    fi

    if [ "$NEED_INSTALL" = "true" ]; then
        echo -e "${YELLOW}[*] 检测到缺失系统级交叉编译依赖，准备安装 (需要 sudo 权限)...${NC}"
        case $OS in
            ubuntu|debian|kali)
                sudo apt-get update
                sudo apt-get install -y build-essential gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 \
                    gcc-mingw-w64-i686 g++-mingw-w64-i686 \
                    gcc-aarch64-linux-gnu g++-aarch64-linux-gnu \
                    musl-tools musl-dev \
                    wget curl git pkg-config libssl-dev perl make unzip
                ;;
            centos|fedora|rhel)
                sudo yum groupinstall -y "Development Tools"
                sudo yum install -y mingw64-gcc mingw32-gcc wget curl git openssl-devel perl make unzip
                ;;
            *)
                echo -e "${YELLOW}[!] 未能识别的操作系统: $OS，请手动安装交叉编译器。${NC}"
                ;;
        esac
    else
        echo -e "${GREEN}[+ ] 系统级交叉编译工具链已就绪。${NC}"
    fi
}

# ... (Go and Rust installation code remains) ...

# 安装交叉编译组件
echo -e "${YELLOW}[*] 正在配置交叉编译工具链 (Multi-Platform)...${NC}"
install_dependencies
rustup target add x86_64-pc-windows-gnu
rustup target add i686-pc-windows-gnu
rustup target add x86_64-unknown-linux-gnu
rustup target add aarch64-unknown-linux-gnu

# 1.1 安装 Donut (用于生成真正的 Shellcode 模板)
if ! command -v donut &> /dev/null && [ ! -f "/usr/local/bin/donut" ]; then
    echo -e "${YELLOW}[*] 正在从源码安装 Donut (采用更稳健的下载方式)...${NC}"
    # 清理旧目录
    rm -rf /tmp/donut_src /tmp/donut.zip
    
    # 尝试使用 wget 下载 ZIP (比 git clone 在某些环境下更稳定)
    # 优先使用加速链接，失败则使用官方链接
    if ! wget --timeout=15 --tries=3 -O /tmp/donut.zip https://codeload.github.com/TheWover/donut/zip/refs/heads/master; then
        echo -e "${YELLOW}[!] 加速链接失败，尝试官方链接...${NC}"
        wget --timeout=15 --tries=3 -O /tmp/donut.zip https://github.com/TheWover/donut/archive/refs/heads/master.zip
    fi

    # 解压并编译
    mkdir -p /tmp/donut_src
    if command -v unzip &> /dev/null; then
        unzip -q /tmp/donut.zip -d /tmp/donut_src
    else
        sudo apt-get install -y unzip || sudo yum install -y unzip
        unzip -q /tmp/donut.zip -d /tmp/donut_src
    fi
    
    # 进入实际代码目录 (zip 解压后通常带一层 master 目录)
    cd /tmp/donut_src/donut-master || cd /tmp/donut_src/donut-*
    make
    sudo cp donut /usr/local/bin/
    cd - && rm -rf /tmp/donut_src /tmp/donut.zip
    echo -e "${GREEN}[+ ] Donut 安装完成。${NC}"
else
    echo -e "${GREEN}[+ ] Donut 已经就绪。${NC}"
fi

# 1.3 安装 UPX (用于减少载荷体积)
if ! command -v upx &> /dev/null; then
    echo -e "${YELLOW}[*] 正在安装 UPX 压缩工具...${NC}"
    case $OS in
        ubuntu|debian|kali) sudo apt-get update && sudo apt-get install -y upx-ucl ;;
        centos|fedora|rhel) sudo yum install -y upx ;;
    esac
else
    echo -e "${GREEN}[+ ] UPX 已经就绪。${NC}"
fi

# 1.2 配置国内代理/镜像 (加速依赖下载)
echo -e "${YELLOW}[*] 正在优化网络代理设置 (国内加速模式)...${NC}"

# Go Proxy
export GOPROXY=https://goproxy.cn,direct
echo 'export GOPROXY=https://goproxy.cn,direct' >> ~/.bashrc

# Cargo Registry Mirror (USTC)
mkdir -p ~/.cargo
cat <<EOF > ~/.cargo/config.toml
[source.crates-io]
replace-with = 'ustc'

[source.ustc]
registry = "sparse+https://mirrors.ustc.edu.cn/crates.io-index/"
EOF

# NPM Registry
if command -v npm &> /dev/null; then
    npm config set registry https://registry.npmmirror.com
fi

# 2. 准备目录
echo -e "${YELLOW}[*] 正在初始化存储目录...${NC}"
mkdir -p server/storage/payloads
mkdir -p server/storage/backups
mkdir -p server/assets

# 3. 编译前端 (如果需要)
if [ ! -d "server/ui" ]; then
    echo -e "${YELLOW}[*] 未检测到前端 UI 目录，尝试从源码构建...${NC}"
    # 查找前端源码目录 (支持 Dashboard 或 server/frontend-v2)
    DASHBOARD_DIR=""
    if [ -d "Dashboard" ]; then DASHBOARD_DIR="Dashboard"
    elif [ -d "server/frontend-v2" ]; then DASHBOARD_DIR="server/frontend-v2"
    fi

    if [ -n "$DASHBOARD_DIR" ]; then
        if ! command -v npm &> /dev/null; then
            echo -e "${YELLOW}[!] 未检测到 NodeJS/NPM，尝试从系统安装...${NC}"
            case $OS in
                ubuntu|debian|kali) sudo apt-get install -y nodejs npm ;;
                centos|fedora|rhel) sudo yum install -y nodejs npm ;;
            esac
        fi
        
        if command -v npm &> /dev/null; then
            cd "$DASHBOARD_DIR"
            npm config set registry https://registry.npmmirror.com
            npm install
            npm run build
            
            # 智能查找 dist 目录 (Vite 可能输出到 ./dist 或 ../dist)
            ACTUAL_DIST_DIR=""
            if [ -d "dist" ]; then ACTUAL_DIST_DIR="dist"
            elif [ -d "../dist" ]; then ACTUAL_DIST_DIR="../dist"
            fi

            if [ -n "$ACTUAL_DIST_DIR" ]; then
                echo -e "${GREEN}[*] 发现构建目录: $ACTUAL_DIST_DIR，正在同步至 server/ui...${NC}"
                mkdir -p "$SCRIPT_DIR/server/ui/"
                cp -r $ACTUAL_DIST_DIR/* "$SCRIPT_DIR/server/ui/"
            else
                echo -e "${RED}[!] 错误: 未能找到编译产物目录 (dist)。${NC}"
            fi
            cd "$SCRIPT_DIR"
        else
            echo -e "${RED}[!] 错误: Node.js 安装失败，无法构建 UI。${NC}"
        fi
    else
        echo -e "${YELLOW}[!] 警告: 未找到 Dashboard 源码，请确保 UI 目录已预编译。${NC}"
    fi
fi

# 4. 生成初始模板 (可选)
if [ ! -f "server/assets/client_template_windows.exe" ] || [ ! -f "server/assets/client_template_linux" ]; then
    echo -e "${YELLOW}[*] 未检测到基础模板，建议执行首次生成...${NC}"
    read -p "是否现在编译多平台免杀模板? (耗时较长) [y/N]: " build_choice
    if [[ "$build_choice" =~ ^[Yy]$ ]]; then
        chmod +x compile_linux.sh compile_windows.sh
        ./compile_linux.sh
        ./compile_windows.sh
    else
        echo -e "${YELLOW}[!] 已跳过模板生成。注意：无模板时‘二进制补丁’模式将不可用。${NC}"
    fi
else
    echo -e "${GREEN}[+ ] 基础模板已存在。${NC}"
    read -p "是否重新编译模板? (如修改了 Client 源码需要重编) [y/N]: " rebuild_choice
    if [[ "$rebuild_choice" =~ ^[Yy]$ ]]; then
        chmod +x compile_linux.sh compile_windows.sh
        ./compile_linux.sh
        ./compile_windows.sh
    fi
fi

# 5. 启动服务端
echo -e "${GREEN}[+ ] 环境初始化完成，准备启动程序...${NC}"
echo -e "${GREEN}[+ ] 控制终端: http://127.0.0.1:9999 ${NC}"

cd server
echo -e "${YELLOW}[*] 正在整理并下载依赖 (此过程在首次运行时可能较慢)...${NC}"
go mod tidy
go mod download
echo -e "${YELLOW}[*] 正在以生产模式编译服务端 (Hardened Build)...${NC}"
if go build -v -ldflags="-s -w" -buildvcs=false -trimpath -o cupcake-server .; then
    echo -e "${GREEN}[+ ] 编译成功，启动程序...${NC}"
    ./cupcake-server
else
    echo -e "${RED}[!] 错误: 服务端编译失败。${NC}"
    exit 1
fi
