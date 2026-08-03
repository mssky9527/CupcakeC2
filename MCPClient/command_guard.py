"""
Cupcake C2 - MCP 命令禁止模块 (Command Guard)
==============================================
针对 Shell 执行和文件管理的安全过滤层。
所有通过 MCP 发出的命令和文件操作都必须经过此模块审查。
被禁止的命令将被打回，返回"无法执行"提示。
"""

import re
import os
import json
from dataclasses import dataclass, field
from typing import Optional


@dataclass
class GuardResult:
    """命令审查结果"""
    allowed: bool
    reason: str = ""
    matched_rule: str = ""
    category: str = ""  # "shell" | "file" | "path" | "system"


# ============================================================
# 默认禁止规则配置
# ============================================================

# Shell 命令黑名单 (正则表达式, 不区分大小写)
SHELL_BLOCKED_PATTERNS: list[dict] = [
    # --- 系统破坏性命令 ---
    {
        "pattern": r"rm\s+(-[a-zA-Z]*f[a-zA-Z]*\s+)?(-[a-zA-Z]*r[a-zA-Z]*\s+)?/\s*$",
        "reason": "禁止删除根目录",
        "category": "system_destructive",
    },
    {
        "pattern": r"rm\s+-[a-zA-Z]*r[a-zA-Z]*f[a-zA-Z]*\s+/(bin|sbin|usr|etc|var|lib|boot|dev|proc|sys)\b",
        "reason": "禁止删除系统关键目录",
        "category": "system_destructive",
    },
    {
        "pattern": r"rm\s+-[a-zA-Z]*f[a-zA-Z]*r[a-zA-Z]*\s+/(bin|sbin|usr|etc|var|lib|boot|dev|proc|sys)\b",
        "reason": "禁止删除系统关键目录",
        "category": "system_destructive",
    },
    {
        "pattern": r"del\s+/[a-zA-Z]*s[a-zA-Z]*\s+/[a-zA-Z]*f[a-zA-Z]*\s+[A-Z]:\\(Windows|WINNT|System32|Program Files)\b",
        "reason": "禁止删除 Windows 系统目录",
        "category": "system_destructive",
    },
    {
        "pattern": r"del\s+/[a-zA-Z]*f[a-zA-Z]*\s+/[a-zA-Z]*s[a-zA-Z]*\s+[A-Z]:\\(Windows|WINNT|System32|Program Files)\b",
        "reason": "禁止删除 Windows 系统目录",
        "category": "system_destructive",
    },
    {
        "pattern": r"rd\s+/[a-zA-Z]*s[a-zA-Z]*\s+/[a-zA-Z]*q[a-zA-Z]*\s+[A-Z]:\\(Windows|WINNT|System32)\b",
        "reason": "禁止删除 Windows 系统目录",
        "category": "system_destructive",
    },
    {
        "pattern": r"format\s+[A-Z]:",
        "reason": "禁止格式化磁盘分区",
        "category": "system_destructive",
    },
    {
        "pattern": r"mkfs(\.\w+)?\s+/dev/",
        "reason": "禁止格式化磁盘分区",
        "category": "system_destructive",
    },
    {
        "pattern": r"dd\s+.*of=/dev/(sd|hd|nvme|vd)",
        "reason": "禁止直接写入块设备",
        "category": "system_destructive",
    },
    # --- 关机/重启命令 ---
    {
        "pattern": r"\b(shutdown|halt|poweroff|reboot|init\s+[06])\b",
        "reason": "禁止执行关机/重启操作",
        "category": "system_control",
    },
    {
        "pattern": r"\bshutdown\s+/[sr]",
        "reason": "禁止执行关机/重启操作",
        "category": "system_control",
    },
    # --- 危险权限操作 ---
    {
        "pattern": r"chmod\s+(-R\s+)?777\s+/",
        "reason": "禁止对根目录递归修改权限",
        "category": "permission_abuse",
    },
    {
        "pattern": r"chown\s+(-R\s+)?\S+\s+/(bin|sbin|usr|etc|var|lib|boot)\b",
        "reason": "禁止修改系统目录所有权",
        "category": "permission_abuse",
    },
    {
        "pattern": r"icacls\s+[A-Z]:\\(Windows|System32)\s+/grant\s+Everyone",
        "reason": "禁止对系统目录授予所有人权限",
        "category": "permission_abuse",
    },
    # --- 网络破坏 ---
    {
        "pattern": r"iptables\s+-F",
        "reason": "禁止清空防火墙规则",
        "category": "network_destructive",
    },
    {
        "pattern": r"netsh\s+advfirewall\s+set\s+allprofiles\s+state\s+off",
        "reason": "禁止关闭系统防火墙",
        "category": "network_destructive",
    },
    # --- 数据库破坏 ---
    {
        "pattern": r"(drop|truncate)\s+(database|table)\b",
        "reason": "禁止删除/清空数据库或数据表",
        "category": "data_destructive",
    },
    {
        "pattern": r"delete\s+from\s+\S+\s*(;|$)",
        "reason": "禁止无条件删除数据库记录",
        "category": "data_destructive",
    },
    # --- 自毁/清除命令 ---
    {
        "pattern": r"self_destruct|self[-_]?destruct",
        "reason": "禁止通过 MCP 触发 Agent 自毁",
        "category": "agent_destructive",
    },
    # --- Fork 炸弹 ---
    {
        "pattern": r":\(\)\s*\{\s*:\s*\|\s*:\s*&\s*\}\s*;?\s*:",
        "reason": "禁止执行 Fork 炸弹",
        "category": "system_destructive",
    },
    {
        "pattern": r"%0\s*\|\s*%0",
        "reason": "禁止执行 Windows Fork 炸弹",
        "category": "system_destructive",
    },
]

# 文件操作黑名单 (针对文件路径的正则)
FILE_BLOCKED_PATTERNS: list[dict] = [
    # Windows 系统关键路径
    {
        "pattern": r"^[A-Z]:\\(Windows|WINNT)(\\|$)",
        "reason": "禁止操作 Windows 系统目录",
        "category": "protected_path",
    },
    {
        "pattern": r"^[A-Z]:\\(Windows|WINNT)\\System32(\\|$)",
        "reason": "禁止操作 System32 目录",
        "category": "protected_path",
    },
    {
        "pattern": r"^[A-Z]:\\(Windows|WINNT)\\SysWOW64(\\|$)",
        "reason": "禁止操作 SysWOW64 目录",
        "category": "protected_path",
    },
    {
        "pattern": r"^[A-Z]:\\(Program Files|Program Files \(x86\))(\\|$)",
        "reason": "禁止操作程序安装目录",
        "category": "protected_path",
    },
    {
        "pattern": r"^[A-Z]:\\\$Recycle\.Bin",
        "reason": "禁止操作回收站",
        "category": "protected_path",
    },
    {
        "pattern": r"^[A-Z]:\\Boot(\\|$)",
        "reason": "禁止操作引导分区",
        "category": "protected_path",
    },
    # Linux 系统关键路径
    {
        "pattern": r"^/(bin|sbin|usr/bin|usr/sbin|usr/lib|lib|lib64)(/|$)",
        "reason": "禁止操作系统二进制目录",
        "category": "protected_path",
    },
    {
        "pattern": r"^/boot(/|$)",
        "reason": "禁止操作引导目录",
        "category": "protected_path",
    },
    {
        "pattern": r"^/dev(/|$)",
        "reason": "禁止操作设备文件",
        "category": "protected_path",
    },
    {
        "pattern": r"^/proc(/|$)",
        "reason": "禁止操作进程文件系统",
        "category": "protected_path",
    },
    {
        "pattern": r"^/sys(/|$)",
        "reason": "禁止操作系统内核接口",
        "category": "protected_path",
    },
    {
        "pattern": r"^/etc/(passwd|shadow|sudoers|fstab|hosts)$",
        "reason": "禁止操作系统关键配置文件",
        "category": "protected_path",
    },
]

# 文件删除操作额外限制 (比读取更严格)
FILE_DELETE_BLOCKED_PATTERNS: list[dict] = [
    {
        "pattern": r"^/etc(/|$)",
        "reason": "禁止删除 /etc 下任何文件",
        "category": "protected_path",
    },
    {
        "pattern": r"^/var(/|$)",
        "reason": "禁止删除 /var 下任何文件",
        "category": "protected_path",
    },
    {
        "pattern": r"^[A-Z]:\\(Users|用户)\\[^\\]+\\(Desktop|桌面|Documents|文档)(\\|$)",
        "reason": "禁止删除用户重要文档目录",
        "category": "protected_path",
    },
]


class CommandGuard:
    """
    MCP 命令安全网关。
    所有 shell 命令和文件操作在执行前必须通过此网关审查。
    """

    def __init__(self, config_path: Optional[str] = None):
        """
        初始化命令禁止模块。

        Args:
            config_path: 可选的自定义规则配置文件路径 (JSON)。
                         如果提供，将合并自定义规则到默认规则中。
        """
        self.shell_patterns = self._compile_patterns(SHELL_BLOCKED_PATTERNS)
        self.file_patterns = self._compile_patterns(FILE_BLOCKED_PATTERNS)
        self.file_delete_patterns = self._compile_patterns(FILE_DELETE_BLOCKED_PATTERNS)
        self.custom_patterns: list[tuple] = []
        # Guard is mandatory for MCP. It cannot be disabled by config; a config
        # that sets "enabled": false is rejected so the guard never fail-opens.
        self.enabled = True
        self.log_blocked: list[dict] = []  # 记录被拦截的命令

        # 加载自定义配置
        if config_path and os.path.exists(config_path):
            self._load_custom_config(config_path)

    def _compile_patterns(self, patterns: list[dict]) -> list[tuple]:
        """预编译正则表达式以提高性能"""
        compiled = []
        for p in patterns:
            try:
                regex = re.compile(p["pattern"], re.IGNORECASE)
                compiled.append((regex, p["reason"], p["category"]))
            except re.error:
                # 跳过无效正则，不中断初始化
                continue
        return compiled

    def _load_custom_config(self, config_path: str):
        """加载自定义规则配置。配置损坏或尝试禁用 Guard 时 fail-closed。"""
        try:
            with open(config_path, "r", encoding="utf-8") as f:
                config = json.load(f)
        except (json.JSONDecodeError, IOError) as e:
            # 配置文件损坏时 fail-closed：保留默认规则，但记录错误。
            # 不允许通过损坏配置让 Guard 失效。
            self._config_error = f"guard config load failed: {e}; using built-in rules"
            return

        # Guard is mandatory. A config that tries to disable it is rejected.
        if config.get("enabled") is False:
            self._config_error = "guard cannot be disabled via config; ignoring 'enabled: false'"
            return

        self._config_error = None

        # 加载自定义 shell 黑名单
        for rule in config.get("shell_blocked", []):
            try:
                regex = re.compile(rule["pattern"], re.IGNORECASE)
                self.custom_patterns.append(
                    (regex, rule.get("reason", "自定义规则拦截"), rule.get("category", "custom"))
                )
            except (re.error, KeyError):
                continue

        # 加载自定义文件路径黑名单
        for rule in config.get("file_blocked", []):
            try:
                regex = re.compile(rule["pattern"], re.IGNORECASE)
                self.file_patterns.append(
                    (regex, rule.get("reason", "自定义路径规则拦截"), rule.get("category", "custom"))
                )
            except (re.error, KeyError):
                continue

    def check_shell_command(self, cmd: str) -> GuardResult:
        """
        审查 Shell 命令是否允许执行。
        
        Args:
            cmd: 待审查的 shell 命令字符串
            
        Returns:
            GuardResult: 审查结果，allowed=True 表示放行
        """
        if not self.enabled:
            return GuardResult(allowed=True)

        if not cmd or not cmd.strip():
            return GuardResult(allowed=False, reason="空命令", matched_rule="empty_command", category="shell")

        cmd_stripped = cmd.strip()

        # 检查内置 shell 黑名单
        for regex, reason, category in self.shell_patterns:
            if regex.search(cmd_stripped):
                self._log_blocked("shell", cmd_stripped, reason, category)
                return GuardResult(
                    allowed=False,
                    reason=reason,
                    matched_rule=regex.pattern,
                    category=category,
                )

        # 检查自定义规则
        for regex, reason, category in self.custom_patterns:
            if regex.search(cmd_stripped):
                self._log_blocked("shell", cmd_stripped, reason, category)
                return GuardResult(
                    allowed=False,
                    reason=reason,
                    matched_rule=regex.pattern,
                    category=category,
                )

        # 检查命令中是否包含被禁止的文件路径 (针对 rm/del/rd 等删除命令)
        delete_cmd_check = self._check_delete_command_paths(cmd_stripped)
        if not delete_cmd_check.allowed:
            return delete_cmd_check

        return GuardResult(allowed=True)

    def check_file_operation(self, path: str, operation: str = "read") -> GuardResult:
        """
        审查文件操作是否允许执行。
        
        Args:
            path: 目标文件/目录路径
            operation: 操作类型 ("read", "list", "delete", "upload", "download")
            
        Returns:
            GuardResult: 审查结果
        """
        if not self.enabled:
            return GuardResult(allowed=True)

        if not path or not path.strip():
            return GuardResult(allowed=False, reason="空路径", matched_rule="empty_path", category="file")

        path_normalized = path.strip().replace("/", os.sep) if os.sep == "\\" else path.strip()

        # 所有文件操作都检查基础黑名单
        for regex, reason, category in self.file_patterns:
            if regex.search(path.strip()):
                self._log_blocked("file", f"{operation}: {path}", reason, category)
                return GuardResult(
                    allowed=False,
                    reason=reason,
                    matched_rule=regex.pattern,
                    category=category,
                )

        # 删除操作额外检查更严格的规则
        if operation == "delete":
            for regex, reason, category in self.file_delete_patterns:
                if regex.search(path.strip()):
                    self._log_blocked("file", f"delete: {path}", reason, category)
                    return GuardResult(
                        allowed=False,
                        reason=reason,
                        matched_rule=regex.pattern,
                        category=category,
                    )

        return GuardResult(allowed=True)

    def check_file_delete_batch(self, paths: list[str]) -> GuardResult:
        """
        批量审查文件删除操作。
        
        Args:
            paths: 待删除的文件路径列表
            
        Returns:
            GuardResult: 如果任何一个路径被禁止，则整体拒绝
        """
        if not self.enabled:
            return GuardResult(allowed=True)

        for p in paths:
            result = self.check_file_operation(p, operation="delete")
            if not result.allowed:
                return result

        return GuardResult(allowed=True)

    def _check_delete_command_paths(self, cmd: str) -> GuardResult:
        """
        检查删除类命令 (rm, del, rd, rmdir) 中的目标路径是否受保护。
        """
        # 提取 rm 命令的目标路径
        rm_match = re.findall(r'\brm\b\s+(?:-[a-zA-Z]+\s+)*(.+)', cmd, re.IGNORECASE)
        for targets in rm_match:
            for target in targets.split():
                target = target.strip("'\"")
                result = self.check_file_operation(target, operation="delete")
                if not result.allowed:
                    self._log_blocked("shell_path", cmd, result.reason, result.category)
                    return result

        # 提取 del/rd/rmdir 命令的目标路径
        del_match = re.findall(r'\b(?:del|erase|rd|rmdir)\b\s+(?:/[a-zA-Z]+\s+)*(.+)', cmd, re.IGNORECASE)
        for targets in del_match:
            for target in targets.split():
                target = target.strip("'\"")
                result = self.check_file_operation(target, operation="delete")
                if not result.allowed:
                    self._log_blocked("shell_path", cmd, result.reason, result.category)
                    return result

        return GuardResult(allowed=True)

    def _log_blocked(self, op_type: str, detail: str, reason: str, category: str):
        """记录被拦截的操作"""
        import time
        self.log_blocked.append({
            "timestamp": time.strftime("%Y-%m-%d %H:%M:%S"),
            "type": op_type,
            "detail": detail[:200],  # 截断过长命令
            "reason": reason,
            "category": category,
        })
        # 保留最近 100 条记录
        if len(self.log_blocked) > 100:
            self.log_blocked = self.log_blocked[-100:]

    def get_blocked_log(self) -> list[dict]:
        """获取被拦截操作的日志"""
        return self.log_blocked.copy()

    def get_status(self) -> dict:
        """获取模块状态信息"""
        return {
            "enabled": self.enabled,
            "shell_rules_count": len(self.shell_patterns),
            "file_rules_count": len(self.file_patterns),
            "file_delete_rules_count": len(self.file_delete_patterns),
            "custom_rules_count": len(self.custom_patterns),
            "total_blocked_count": len(self.log_blocked),
        }

    def format_rejection(self, result: GuardResult) -> str:
        """
        格式化拒绝消息，返回给 MCP 调用方。
        """
        return (
            f"[命令已拦截 - 无法执行]\n"
            f"原因: {result.reason}\n"
            f"分类: {result.category}\n"
            f"规则: {result.matched_rule}\n"
            f"---\n"
            f"该操作被 MCP 安全网关 (Command Guard) 拦截。\n"
            f"如需执行此操作，请通过 C2 Web 控制台直接操作。"
        )


# ============================================================
# 全局单例 (供 client.py 直接使用)
# ============================================================

_guard_instance: Optional[CommandGuard] = None


def get_guard(config_path: Optional[str] = None) -> CommandGuard:
    """获取全局 CommandGuard 单例"""
    global _guard_instance
    if _guard_instance is None:
        _guard_instance = CommandGuard(config_path=config_path)
    return _guard_instance
