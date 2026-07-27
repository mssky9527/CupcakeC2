// 指数退避重连机制
//
// 实现指数退避算法，用于在连接失败时控制重连间隔。
// 初始延迟为 1 秒，每次失败后延迟时间翻倍，最大延迟为 60 秒。

use std::time::Duration;

/// Apply ±jitter_percent random variation to a delay (minimum 1ms when base > 0).
pub fn apply_delay_jitter(base: Duration, jitter_percent: u32) -> Duration {
    if jitter_percent == 0 || base.is_zero() {
        return base;
    }
    let base_ms = base.as_millis() as u64;
    if base_ms == 0 {
        return base;
    }
    let span = (base_ms * jitter_percent as u64) / 100;
    if span == 0 {
        return base;
    }
    // Mix LCG with time so consecutive calls differ even if RNG seed is fixed
    let r = crate::utils::next_u32_secure() as u64 % (span * 2 + 1);
    let half = span;
    let jittered = base_ms.saturating_sub(half).saturating_add(r).max(1);
    Duration::from_millis(jittered)
}

/// 指数退避策略
/// 
/// 用于控制重连间隔时间，实现指数增长的延迟策略。
/// 
/// # 算法
/// 
/// - 初始延迟：1 秒
/// - 增长因子：2（每次失败后延迟时间翻倍）
/// - 最大延迟：60 秒
/// 
/// # 示例
/// 
/// ```
/// use c2_client_agent::ExponentialBackoff;
/// use std::time::Duration;
/// 
/// let mut backoff = ExponentialBackoff::new();
/// 
/// // 第一次重连：等待 1 秒
/// assert_eq!(backoff.next_delay(), Duration::from_secs(1));
/// 
/// // 第二次重连：等待 2 秒
/// assert_eq!(backoff.next_delay(), Duration::from_secs(2));
/// 
/// // 连接成功后重置
/// backoff.reset();
/// assert_eq!(backoff.next_delay(), Duration::from_secs(1));
/// ```
#[derive(Debug, Clone)]
pub struct ExponentialBackoff {
    /// 当前延迟时间
    current_delay: Duration,
    /// 最大延迟时间
    max_delay: Duration,
    /// 增长倍数
    multiplier: u32,
}

impl ExponentialBackoff {
    /// 创建新的指数退避策略
    /// 
    /// 初始延迟为 1 秒，最大延迟为 60 秒，增长因子为 2。
    pub fn new() -> Self {
        Self {
            current_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            multiplier: 2,
        }
    }
    
    /// 获取下一次重连的延迟时间（带 ±20% 抖动，避免同步 beacon）
    ///
    /// 该方法返回当前的延迟时间（加入抖动），并将内部状态更新为下一次的延迟时间。
    /// 延迟时间按指数增长，直到达到最大值。
    ///
    /// # 返回值
    ///
    /// 返回当前应该等待的时间长度（含抖动）。
    pub fn next_delay(&mut self) -> Duration {
        let base = self.current_delay;

        // 计算下一次的延迟时间（当前延迟 * 倍数）
        let next = self.current_delay.as_secs() * self.multiplier as u64;

        // 确保不超过最大延迟
        if next >= self.max_delay.as_secs() {
            self.current_delay = self.max_delay;
        } else {
            self.current_delay = Duration::from_secs(next);
        }

        // ±20% jitter so agents do not reconnect in lockstep
        apply_delay_jitter(base, 20)
    }

    /// Base delay without advancing state (for tests/diagnostics).
    pub fn next_delay_no_jitter_preview(&self) -> Duration {
        self.current_delay
    }
    
    /// 重置延迟时间
    /// 
    /// 将延迟时间重置为初始值（1 秒）。
    /// 通常在连接成功后调用，以便下次连接失败时从初始延迟开始。
    pub fn reset(&mut self) {
        self.current_delay = Duration::from_secs(1);
    }
    
    /// 获取当前延迟时间（不更新状态）
    /// 
    /// 该方法仅用于查看当前的延迟时间，不会修改内部状态。
    pub fn current(&self) -> Duration {
        self.current_delay
    }
}

impl Default for ExponentialBackoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_delay() {
        let backoff = ExponentialBackoff::new();
        assert_eq!(backoff.current(), Duration::from_secs(1));
    }

    #[test]
    fn test_exponential_growth() {
        let mut backoff = ExponentialBackoff::new();

        // Internal base grows 1,2,4,...; returned delay includes ±20% jitter
        let d1 = backoff.next_delay();
        assert!(d1.as_millis() >= 800 && d1.as_millis() <= 1200);
        assert_eq!(backoff.current(), Duration::from_secs(2));

        let d2 = backoff.next_delay();
        assert!(d2.as_millis() >= 1600 && d2.as_millis() <= 2400);
        assert_eq!(backoff.current(), Duration::from_secs(4));

        let _ = backoff.next_delay(); // base 4 → next 8
        let _ = backoff.next_delay(); // base 8 → next 16
        let _ = backoff.next_delay(); // base 16 → next 32
        let _ = backoff.next_delay(); // base 32 → next 60
        assert_eq!(backoff.current(), Duration::from_secs(60));

        let dmax = backoff.next_delay();
        assert!(dmax.as_secs() >= 48 && dmax.as_secs() <= 72);
        assert_eq!(backoff.current(), Duration::from_secs(60));
    }

    #[test]
    fn test_max_delay_cap() {
        let mut backoff = ExponentialBackoff::new();

        // Internal base caps at 60s; returned delay may include ±20% jitter (≤72s)
        for _ in 0..20 {
            let delay = backoff.next_delay();
            assert!(delay.as_secs() <= 72);
            assert!(backoff.current().as_secs() <= 60);
        }
    }

    #[test]
    fn test_reset() {
        let mut backoff = ExponentialBackoff::new();

        // 增长到较大的延迟
        backoff.next_delay(); // 1
        backoff.next_delay(); // 2
        backoff.next_delay(); // 4
        backoff.next_delay(); // 8

        // 重置后应该回到初始值
        backoff.reset();
        assert_eq!(backoff.current(), Duration::from_secs(1));
        let d = backoff.next_delay();
        assert!(d.as_millis() >= 800 && d.as_millis() <= 1200);
    }

    #[test]
    fn test_reset_after_max() {
        let mut backoff = ExponentialBackoff::new();
        
        // 增长到最大值
        for _ in 0..10 {
            backoff.next_delay();
        }
        
        assert_eq!(backoff.current(), Duration::from_secs(60));
        
        // 重置后应该回到初始值
        backoff.reset();
        assert_eq!(backoff.current(), Duration::from_secs(1));
    }

    #[test]
    fn test_current_does_not_modify_state() {
        let mut backoff = ExponentialBackoff::new();

        // current() 不应该修改状态
        assert_eq!(backoff.current(), Duration::from_secs(1));
        assert_eq!(backoff.current(), Duration::from_secs(1));
        assert_eq!(backoff.current(), Duration::from_secs(1));

        // next_delay() 应该修改状态
        let _ = backoff.next_delay();
        assert_eq!(backoff.current(), Duration::from_secs(2));
    }

    #[test]
    fn test_default_trait() {
        let backoff = ExponentialBackoff::default();
        assert_eq!(backoff.current(), Duration::from_secs(1));
    }

    #[test]
    fn test_delay_sequence_bases() {
        let mut backoff = ExponentialBackoff::new();
        // After each next_delay, internal base should follow exponential sequence
        let expected_bases = vec![2, 4, 8, 16, 32, 60, 60, 60, 60];
        for expected in expected_bases {
            let _ = backoff.next_delay();
            assert_eq!(backoff.current(), Duration::from_secs(expected));
        }
    }

    #[test]
    fn test_jitter_varies() {
        let base = Duration::from_secs(10);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..40 {
            seen.insert(apply_delay_jitter(base, 20).as_millis());
        }
        assert!(
            seen.len() > 1,
            "jitter must produce more than one distinct delay"
        );
    }

    #[test]
    fn test_multiple_resets() {
        let mut backoff = ExponentialBackoff::new();

        // 第一轮
        let _ = backoff.next_delay();
        let _ = backoff.next_delay();
        assert_eq!(backoff.current(), Duration::from_secs(4));

        // 重置
        backoff.reset();
        assert_eq!(backoff.current(), Duration::from_secs(1));

        // 第二轮
        let _ = backoff.next_delay();
        assert_eq!(backoff.current(), Duration::from_secs(2));

        // 再次重置
        backoff.reset();
        assert_eq!(backoff.current(), Duration::from_secs(1));
    }

    #[test]
    fn test_clone() {
        let mut backoff1 = ExponentialBackoff::new();
        backoff1.next_delay(); // 1
        backoff1.next_delay(); // 2
        
        // 克隆应该保持相同的状态
        let backoff2 = backoff1.clone();
        assert_eq!(backoff1.current(), backoff2.current());
        assert_eq!(backoff2.current(), Duration::from_secs(4));
    }
}
