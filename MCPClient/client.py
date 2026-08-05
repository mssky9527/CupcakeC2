"""
Cupcake C2 - MCP Server (扩展版)
=================================
通过 MCP (Model Context Protocol) 将 C2 功能暴露为 AI 可调用工具。
集成 Command Guard 安全网关，拦截危险的 Shell 和文件操作。

功能:
- 客户端管理 (列表/历史/仪表盘)
- Shell 命令执行 (带安全过滤)
- 文件系统操作 (列表/读取/删除, 带路径保护)
- 进程管理 (列表/终止)
- 隧道管理 (列表/启动/停止)
- 监听器管理 (列表)
- 武器库插件 (列表/执行/结果查询)
- 安全网关状态查询
"""

import asyncio
import os
import sys
import json
import logging
from typing import Optional

import requests
from mcp.server import Server, NotificationOptions
from mcp.server.models import InitializationOptions
import mcp.types as types
from mcp.server.stdio import stdio_server

# 导入命令禁止模块
from command_guard import CommandGuard, get_guard, GuardResult

# ============================================================
# 配置 (支持环境变量覆盖)
# ============================================================

C2_SERVER = os.environ.get("C2_SERVER", "http://127.0.0.1:9999/").rstrip("/") + "/"
# MCP token is write-only on the server (revealed once by /api/settings/mcp/rotate-token).
# A missing token is a hard startup failure — never ship a default credential.
API_TOKEN = os.environ.get("C2_API_TOKEN", "")
if not API_TOKEN:
    logging.error("C2_API_TOKEN is required. Rotate it from /api/settings/mcp/rotate-token and export it before starting MCP.")
    sys.exit(1)
REQUEST_TIMEOUT = int(os.environ.get("C2_TIMEOUT", "30"))

# 自定义规则配置文件路径 (可选)
GUARD_CONFIG_PATH = os.environ.get(
    "C2_GUARD_CONFIG",
    os.path.join(os.path.dirname(os.path.abspath(__file__)), "guard_rules.json")
)

# 日志配置 (输出到 stderr，不干扰 stdio 通信)
logging.basicConfig(
    level=logging.INFO,
    format="[%(asctime)s] %(levelname)s: %(message)s",
    stream=sys.stderr,
)
logger = logging.getLogger("cupcake-mcp")

# ============================================================
# 初始化
# ============================================================

server = Server("cupcake-c2", version="2.0.0")
guard: CommandGuard = get_guard(config_path=GUARD_CONFIG_PATH)

logger.info(f"MCP Server 初始化完成 | C2: {C2_SERVER} | Guard: {'启用' if guard.enabled else '禁用'}")
logger.info(f"Guard 规则: Shell={len(guard.shell_patterns)} File={len(guard.file_patterns)} "
            f"Delete={len(guard.file_delete_patterns)} Custom={len(guard.custom_patterns)}")


# ============================================================
# HTTP 请求封装
# ============================================================

def c2_request(method: str, endpoint: str, params: dict = None,
               json_data: dict = None, timeout: int = None) -> str:
    """
    向 C2 Server 发送 API 请求。

    Returns:
        str: JSON 文本。成功时包含 data；失败时包含 ok=false、status、error_code、message。
    """
    url = f"{C2_SERVER}{endpoint.lstrip('/')}"
    headers = {
        "Authorization": f"Bearer {API_TOKEN}",
        "Content-Type": "application/json",
    }
    used_timeout = timeout or REQUEST_TIMEOUT
    try:
        resp = requests.request(
            method, url,
            headers=headers,
            params=params,
            json=json_data,
            timeout=used_timeout,
        )
    except requests.exceptions.Timeout:
        return json.dumps({"ok": False, "status": None, "error_code": "timeout",
                           "message": f"request timeout ({used_timeout}s)", "endpoint": endpoint},
                          ensure_ascii=False)
    except requests.exceptions.ConnectionError as e:
        return json.dumps({"ok": False, "status": None, "error_code": "connection_error",
                           "message": "cannot connect to C2 server", "endpoint": endpoint},
                          ensure_ascii=False)
    except Exception as e:
        return json.dumps({"ok": False, "status": None, "error_code": "client_error",
                           "message": "unexpected client error", "endpoint": endpoint},
                          ensure_ascii=False)

    if resp.status_code == 401:
        return json.dumps({"ok": False, "status": 401, "error_code": "unauthorized",
                           "message": "MCP token rejected (rotate and update C2_API_TOKEN)",
                           "endpoint": endpoint}, ensure_ascii=False)
    if resp.status_code == 403:
        try:
            payload = resp.json()
        except (json.JSONDecodeError, ValueError):
            payload = {}
        code = payload.get("error_code") if isinstance(payload, dict) else None
        if not code:
            code = "mcp_policy_denied"
        return json.dumps({"ok": False, "status": 403, "error_code": code,
                           "message": payload.get("error", "mcp policy denied") if isinstance(payload, dict) else "mcp policy denied",
                           "endpoint": endpoint}, ensure_ascii=False)
    if resp.status_code == 404:
        return json.dumps({"ok": False, "status": 404, "error_code": "not_found",
                           "message": "endpoint or resource not found", "endpoint": endpoint},
                          ensure_ascii=False)
    if resp.status_code >= 500:
        return json.dumps({"ok": False, "status": resp.status_code, "error_code": "server_error",
                           "message": "C2 server error", "endpoint": endpoint},
                          ensure_ascii=False)

    try:
        data = resp.json()
        return json.dumps({"ok": True, "status": resp.status_code, "data": data},
                          ensure_ascii=False, indent=2)
    except (json.JSONDecodeError, ValueError):
        return json.dumps({"ok": True, "status": resp.status_code, "data": resp.text},
                          ensure_ascii=False)


# ============================================================
# 工具定义
# ============================================================

@server.list_tools()
async def handle_list_tools() -> list[types.Tool]:
    """列出所有可用的 C2 MCP 工具"""
    return [
        # --- 客户端管理 ---
        types.Tool(
            name="get_clients",
            description="获取所有在线受控端列表，包含 UUID、IP、主机名、系统信息等",
            inputSchema={"type": "object", "properties": {}},
        ),
        types.Tool(
            name="get_dashboard",
            description="获取 C2 仪表盘概览信息（在线数量、监听器状态等）",
            inputSchema={"type": "object", "properties": {}},
        ),
        types.Tool(
            name="get_history",
            description="获取指定受控端的命令执行历史及结果",
            inputSchema={
                "type": "object",
                "properties": {
                    "uuid": {"type": "string", "description": "受控端 UUID"}
                },
                "required": ["uuid"],
            },
        ),
        # --- Shell 命令 ---
        types.Tool(
            name="send_cmd",
            description="在受控端执行 Shell 指令（受安全网关保护，危险命令会被拦截）",
            inputSchema={
                "type": "object",
                "properties": {
                    "uuid": {"type": "string", "description": "受控端 UUID"},
                    "cmd": {"type": "string", "description": "要执行的 Shell 命令"},
                },
                "required": ["uuid", "cmd"],
            },
        ),
        # --- 文件系统 ---
        types.Tool(
            name="list_files",
            description="列出受控端指定目录的文件列表（受路径保护规则限制）",
            inputSchema={
                "type": "object",
                "properties": {
                    "uuid": {"type": "string", "description": "受控端 UUID"},
                    "path": {"type": "string", "description": "目录路径，默认为当前目录"},
                },
                "required": ["uuid"],
            },
        ),
        types.Tool(
            name="read_file",
            description="读取受控端指定文件的内容（前 50KB 预览）",
            inputSchema={
                "type": "object",
                "properties": {
                    "uuid": {"type": "string", "description": "受控端 UUID"},
                    "path": {"type": "string", "description": "文件完整路径"},
                },
                "required": ["uuid", "path"],
            },
        ),
        types.Tool(
            name="delete_files",
            description="删除受控端指定文件（受严格路径保护，系统目录不可删除）",
            inputSchema={
                "type": "object",
                "properties": {
                    "uuid": {"type": "string", "description": "受控端 UUID"},
                    "paths": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "要删除的文件路径列表",
                    },
                },
                "required": ["uuid", "paths"],
            },
        ),
        # --- 进程管理 ---
        types.Tool(
            name="list_processes",
            description="获取受控端进程列表",
            inputSchema={
                "type": "object",
                "properties": {
                    "uuid": {"type": "string", "description": "受控端 UUID"},
                },
                "required": ["uuid"],
            },
        ),
        types.Tool(
            name="kill_process",
            description="终止受控端指定 PID 的进程",
            inputSchema={
                "type": "object",
                "properties": {
                    "uuid": {"type": "string", "description": "受控端 UUID"},
                    "pid": {"type": "integer", "description": "目标进程 PID"},
                },
                "required": ["uuid", "pid"],
            },
        ),
        # --- 隧道管理 ---
        types.Tool(
            name="list_tunnels",
            description="获取当前活跃的隧道/SOCKS 代理列表",
            inputSchema={"type": "object", "properties": {}},
        ),
        types.Tool(
            name="start_tunnel",
            description="为受控端启动 SOCKS5/HTTP 隧道代理",
            inputSchema={
                "type": "object",
                "properties": {
                    "uuid": {"type": "string", "description": "受控端 UUID"},
                    "port": {"type": "string", "description": "本地监听端口"},
                    "type": {"type": "string", "description": "隧道类型: socks5 或 http", "default": "socks5"},
                    "username": {"type": "string", "description": "认证用户名（可选）"},
                    "password": {"type": "string", "description": "认证密码（可选）"},
                },
                "required": ["uuid", "port"],
            },
        ),
        types.Tool(
            name="stop_tunnel",
            description="停止指定端口的隧道代理",
            inputSchema={
                "type": "object",
                "properties": {
                    "port": {"type": "string", "description": "要停止的隧道端口"},
                },
                "required": ["port"],
            },
        ),
        # --- 监听器管理 ---
        types.Tool(
            name="list_listeners",
            description="获取所有监听器列表及状态",
            inputSchema={"type": "object", "properties": {}},
        ),
        # --- 武器库插件 ---
        types.Tool(
            name="list_plugins",
            description="获取武器库插件列表",
            inputSchema={"type": "object", "properties": {}},
        ),
        types.Tool(
            name="run_plugin",
            description="在指定受控端上运行武器库插件",
            inputSchema={
                "type": "object",
                "properties": {
                    "uuid": {"type": "string", "description": "受控端 UUID"},
                    "plugin_id": {"type": "string", "description": "插件 ID"},
                    "args": {"type": "string", "description": "插件参数"},
                },
                "required": ["uuid", "plugin_id", "args"],
            },
        ),
        types.Tool(
            name="get_plugin_result",
            description="获取插件执行结果（异步轮询）",
            inputSchema={
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "任务 ID (run_plugin 返回)"},
                },
                "required": ["task_id"],
            },
        ),
        # --- 安全网关 ---
        types.Tool(
            name="guard_status",
            description="查询 MCP 安全网关 (Command Guard) 的状态和拦截日志",
            inputSchema={
                "type": "object",
                "properties": {
                    "show_log": {"type": "boolean", "description": "是否显示拦截日志", "default": False},
                },
            },
        ),
    ]


# ============================================================
# 工具调用处理
# ============================================================

@server.call_tool()
async def handle_call_tool(
    name: str, arguments: dict | None
) -> list[types.TextContent]:
    """处理所有 MCP 工具调用，集成 Command Guard 安全审查"""

    if arguments is None:
        arguments = {}

    try:
        result = await _dispatch_tool(name, arguments)
        return [types.TextContent(type="text", text=result)]
    except ValueError as e:
        return [types.TextContent(type="text", text=json.dumps({"error": str(e)}))]
    except Exception as e:
        logger.exception(f"工具调用异常: {name}")
        return [types.TextContent(type="text", text=json.dumps({"error": f"内部错误: {str(e)}"}))]


async def _dispatch_tool(name: str, args: dict) -> str:
    """工具路由分发"""

    # === 客户端管理 ===
    if name == "get_clients":
        return c2_request("GET", "/api/clients")

    elif name == "get_dashboard":
        return c2_request("GET", "/api/dashboard")

    elif name == "get_history":
        uuid_val = args.get("uuid", "")
        if not uuid_val:
            return json.dumps({"error": "缺少参数: uuid"})
        return c2_request("GET", f"/api/clients/history/{uuid_val}")

    # === Shell 命令 (受 Command Guard 保护) ===
    elif name == "send_cmd":
        uuid_val = args.get("uuid", "")
        cmd = args.get("cmd", "")
        if not uuid_val:
            return json.dumps({"error": "缺少参数: uuid"})
        if not cmd:
            return json.dumps({"error": "缺少参数: cmd"})

        # 安全网关审查
        check = guard.check_shell_command(cmd)
        if not check.allowed:
            logger.warning(f"[GUARD] Shell 命令被拦截: {cmd[:100]} | 原因: {check.reason}")
            return guard.format_rejection(check)

        return c2_request("POST", "/api/cmd", json_data={"uuid": uuid_val, "cmd": cmd})

    # === 文件系统 (受路径保护) ===
    elif name == "list_files":
        uuid_val = args.get("uuid", "")
        path = args.get("path", "")
        if not uuid_val:
            return json.dumps({"error": "缺少参数: uuid"})

        # 路径安全审查
        if path:
            check = guard.check_file_operation(path, operation="list")
            if not check.allowed:
                logger.warning(f"[GUARD] 文件列表被拦截: {path} | 原因: {check.reason}")
                return guard.format_rejection(check)

        params = {"uuid": uuid_val}
        if path:
            params["path"] = path
        return c2_request("GET", "/api/files/list", params=params)

    elif name == "read_file":
        uuid_val = args.get("uuid", "")
        path = args.get("path", "")
        if not uuid_val:
            return json.dumps({"error": "缺少参数: uuid"})
        if not path:
            return json.dumps({"error": "缺少参数: path"})

        # 路径安全审查
        check = guard.check_file_operation(path, operation="read")
        if not check.allowed:
            logger.warning(f"[GUARD] 文件读取被拦截: {path} | 原因: {check.reason}")
            return guard.format_rejection(check)

        params = {"uuid": uuid_val, "path": path}
        return c2_request("GET", "/api/files/read", params=params)

    elif name == "delete_files":
        uuid_val = args.get("uuid", "")
        paths = args.get("paths", [])
        if not uuid_val:
            return json.dumps({"error": "缺少参数: uuid"})
        if not paths or not isinstance(paths, list):
            return json.dumps({"error": "缺少参数: paths (需要字符串数组)"})

        # 批量路径安全审查 (删除操作使用更严格规则)
        check = guard.check_file_delete_batch(paths)
        if not check.allowed:
            logger.warning(f"[GUARD] 文件删除被拦截: {paths} | 原因: {check.reason}")
            return guard.format_rejection(check)

        return c2_request("POST", "/api/files/delete", json_data={"uuid": uuid_val, "paths": paths})

    # === 进程管理 ===
    elif name == "list_processes":
        uuid_val = args.get("uuid", "")
        if not uuid_val:
            return json.dumps({"error": "缺少参数: uuid"})
        return c2_request("GET", "/api/processes/list", params={"uuid": uuid_val})

    elif name == "kill_process":
        uuid_val = args.get("uuid", "")
        pid = args.get("pid")
        if not uuid_val:
            return json.dumps({"error": "缺少参数: uuid"})
        if pid is None:
            return json.dumps({"error": "缺少参数: pid"})
        return c2_request("POST", "/api/processes/kill", json_data={"uuid": uuid_val, "pid": int(pid)})

    # === 隧道管理 ===
    elif name == "list_tunnels":
        return c2_request("GET", "/api/tunnel")

    elif name == "start_tunnel":
        uuid_val = args.get("uuid", "")
        port = args.get("port", "")
        if not uuid_val:
            return json.dumps({"error": "缺少参数: uuid"})
        if not port:
            return json.dumps({"error": "缺少参数: port"})

        payload = {
            "uuid": uuid_val,
            "port": str(port),
            "type": args.get("type", "socks5"),
        }
        if args.get("username"):
            payload["username"] = args["username"]
        if args.get("password"):
            payload["password"] = args["password"]

        return c2_request("POST", "/api/tunnel/start", json_data=payload)

    elif name == "stop_tunnel":
        port = args.get("port", "")
        if not port:
            return json.dumps({"error": "缺少参数: port"})
        return c2_request("POST", "/api/tunnel/stop", json_data={"port": str(port)})

    # === 监听器管理 ===
    elif name == "list_listeners":
        return c2_request("GET", "/api/listeners")

    # === 武器库插件 ===
    elif name == "list_plugins":
        return c2_request("GET", "/api/plugins")

    elif name == "run_plugin":
        uuid_val = args.get("uuid", "")
        plugin_id = args.get("plugin_id", "")
        plugin_args = args.get("args", "")
        if not uuid_val:
            return json.dumps({"error": "缺少参数: uuid"})
        if not plugin_id:
            return json.dumps({"error": "缺少参数: plugin_id"})

        return c2_request("POST", "/api/plugins/run", json_data={
            "uuid": uuid_val,
            "plugin_id": plugin_id,
            "args": plugin_args,
        })

    elif name == "get_plugin_result":
        task_id = args.get("task_id", "")
        if not task_id:
            return json.dumps({"error": "缺少参数: task_id"})
        return c2_request("GET", f"/api/plugins/result/{task_id}")

    # === 安全网关状态 ===
    elif name == "guard_status":
        show_log = args.get("show_log", False)
        status = guard.get_status()
        result = {"guard_status": status}
        if show_log:
            result["blocked_log"] = guard.get_blocked_log()
        return json.dumps(result, ensure_ascii=False, indent=2)

    else:
        raise ValueError(f"未知工具: {name}")


# ============================================================
# 启动入口
# ============================================================

async def main():
    """启动 MCP stdio 服务器"""
    logger.info("Cupcake C2 MCP Server v2.0.0 启动中...")
    logger.info(f"安全网关状态: {'启用' if guard.enabled else '禁用'}")

    async with stdio_server() as (read_stream, write_stream):
        init_options = InitializationOptions(
            server_name="cupcake-c2",
            server_version="2.0.0",
            capabilities=server.get_capabilities(
                notification_options=NotificationOptions(tools_changed=True),
                experimental_capabilities={},
            ),
        )
        await server.run(read_stream, write_stream, init_options)


if __name__ == "__main__":
    asyncio.run(main())
