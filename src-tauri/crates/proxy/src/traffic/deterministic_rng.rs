//! 轻量确定性伪随机数，用于让同一规则种子产生可复现的抖动序列。
//!
//! 它不是密码学随机数，不能用于密钥、nonce 或安全令牌；零种子会被规范化，范围退化时
//! 直接返回下界，避免取模异常。

#[derive(Clone, Copy, Debug)]
pub struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    pub fn range_inclusive(&mut self, minimum: u64, maximum: u64) -> u64 {
        if minimum >= maximum {
            return minimum;
        }
        let mut value = self.state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.state = value;
        minimum + value % (maximum - minimum + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::DeterministicRng;

    #[test]
    fn equal_seeds_produce_equal_sequences() {
        let mut left = DeterministicRng::new(42);
        let mut right = DeterministicRng::new(42);
        let left = (0..8)
            .map(|_| left.range_inclusive(10, 100))
            .collect::<Vec<_>>();
        let right = (0..8)
            .map(|_| right.range_inclusive(10, 100))
            .collect::<Vec<_>>();
        assert_eq!(left, right);
    }
}
