use std::fmt;

/// Encapsula uma palavra de instrução de 32 bits do MIPS I.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Instruction(pub u32);

impl Instruction {
    #[inline(always)]
    pub fn opcode(&self) -> u32 { self.0 >> 26 }
    #[inline(always)]
    pub fn rs(&self) -> usize { ((self.0 >> 21) & 0x1f) as usize }
    #[inline(always)]
    pub fn rt(&self) -> usize { ((self.0 >> 16) & 0x1f) as usize }
    #[inline(always)]
    pub fn rd(&self) -> usize { ((self.0 >> 11) & 0x1f) as usize }
    #[inline(always)]
    pub fn shift(&self) -> u32 { (self.0 >> 6) & 0x1f }
    #[inline(always)]
    pub fn subfunction(&self) -> u32 { self.0 & 0x3f }
    #[inline(always)]
    pub fn imm(&self) -> u32 { self.0 & 0xffff }
    #[inline(always)]
    pub fn imm_se(&self) -> u32 { (self.0 as i16) as i32 as u32 }
    #[inline(always)]
    pub fn imm_jump(&self) -> u32 { self.0 & 0x03ff_ffff }
    #[inline(always)]
    pub fn cop_opcode(&self) -> u32 { (self.0 >> 21) & 0x1f }
    #[inline(always)]
    pub fn sys_break_code(&self) -> u32 { (self.0 >> 6) & 0x000f_ffff }
}

impl fmt::Debug for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Instr(0x{:08X} | Op: 0x{:02X}, rs: ${}, rt: ${}, rd: ${}, imm: 0x{:04X})",
            self.0,
            self.opcode(),
            self.rs(),
            self.rt(),
            self.rd(),
            self.imm()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Instruction;

    #[test]
    fn test_lui_decoding() {
        let instr = Instruction(0x3C08_1234);
        assert_eq!(instr.opcode(), 0x0f);
        assert_eq!(instr.rt(), 8);
        assert_eq!(instr.imm(), 0x1234);
    }

    #[test]
    fn test_sign_extension() {
        assert_eq!(Instruction(0x2402_ffff).imm_se(), 0xffff_ffff);
        assert_eq!(Instruction(0x2402_0005).imm_se(), 5);
    }

    #[test]
    fn test_r_type_decoding() {
        let instr = Instruction(0x0085_1021);
        assert_eq!(instr.opcode(), 0);
        assert_eq!(instr.rs(), 4);
        assert_eq!(instr.rt(), 5);
        assert_eq!(instr.rd(), 2);
        assert_eq!(instr.subfunction(), 0x21);
    }

    #[test]
    fn test_jump_decoding() {
        let instr = Instruction(0x0804_0000);
        assert_eq!(instr.opcode(), 2);
        assert_eq!(instr.imm_jump(), 0x0004_0000);
    }
}
