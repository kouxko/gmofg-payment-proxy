/// 跨平台确定性 PRNG。
///
/// 不使用系统随机数，确保同一 Profile seed 与相同包序列在 Windows、macOS、Android
/// 上得到完全相同的故障序列，便于复现现场问题。
#[derive(Clone, Copy, Debug)]
pub(crate) struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    pub(crate) fn new(seed: u64) -> Self {
        // xorshift 的全零状态会永远输出零，因此把用户的零 seed 映射到固定非零值。
        Self {
            state: if seed == 0 {
                0x9e37_79b9_7f4a_7c15
            } else {
                seed
            },
        }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.state = value;
        value.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub(crate) fn hits_basis_points(&mut self, basis_points: u16) -> bool {
        basis_points > 0 && self.next_u64() % 10_000 < u64::from(basis_points)
    }

    pub(crate) fn inclusive(&mut self, maximum: u64) -> u64 {
        if maximum == 0 {
            0
        } else {
            self.next_u64() % maximum.saturating_add(1)
        }
    }
}
