// src/cpu.rs

use crate::bus::Bus;
use crate::gte::Gte;
use crate::instruction::Instruction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Exception {
    Interrupt = 0x00,
    LoadAddressError = 0x04,
    StoreAddressError = 0x05,
    SysCall = 0x08,
    Break = 0x09,
    IllegalInstruction = 0x0A,
    CoprocessorError = 0x0B,
    Overflow = 0x0C,
}

/// Linha do Cache de Instruções (I-Cache) de 4 palavras (16 bytes)
#[derive(Clone, Copy, Debug)]
pub struct ICacheLine {
    pub tag_valid: u32,
    pub words: [u32; 4],
}

impl Default for ICacheLine {
    fn default() -> Self {
        Self {
            tag_valid: 0x0000_0010, // Inicia invalidada (valid_index = 4)
            words: [0; 4],
        }
    }
}

impl ICacheLine {
    #[inline(always)]
    pub fn tag(&self) -> u32 {
        self.tag_valid & 0x7FFF_F000
    }

    #[inline(always)]
    pub fn valid_index(&self) -> u32 {
        (self.tag_valid >> 2) & 0x07
    }

    #[inline(always)]
    pub fn set_tag_valid(&mut self, pc: u32) {
        self.tag_valid = pc & 0x7FFF_F00C;
    }

    #[inline(always)]
    pub fn invalidate(&mut self) {
        self.tag_valid |= 0x10;
    }
}

pub struct Cpu {
    pub pc: u32,
    pub next_pc: u32,
    pub current_pc: u32,
    pub regs: [u32; 32],
    pub hi: u32,
    pub lo: u32,

    // Registradores completos do Coprocessador 0 (System Control)
    pub bpc: u32,       // r3: Breakpoint on execute
    pub bda: u32,       // r5: Breakpoint on data access
    pub jumpdest: u32,  // r6: Memorized jump address
    pub dcic: u32,      // r7: Breakpoint control
    pub bad_vaddr: u32, // r8: Bad Virtual Address
    pub bdam: u32,      // r9: Data access breakpoint mask
    pub bpcm: u32,      // r11: Execute breakpoint mask
    pub sr: u32,        // r12: Status Register
    pub cause: u32,     // r13: Cause Register
    pub epc: u32,       // r14: Exception Program Counter

    // Cache de Instruções de 4 KB (256 linhas x 16 bytes)
    pub icache: Box<[ICacheLine; 256]>,

    // Coprocessador 2 (Geometry Transform Engine)
    pub gte: Gte,

    // Estado do Pipeline MIPS (Load Delay e Branch Delay)
    load: (usize, u32),
    is_branching: bool,
    in_delay_slot: bool,
}

impl Cpu {
    pub fn new() -> Self {
        const RESET_VECTOR: u32 = 0xBFC0_0000;
        Self {
            pc: RESET_VECTOR,
            next_pc: RESET_VECTOR.wrapping_add(4),
            current_pc: 0,
            regs: [0; 32],
            hi: 0,
            lo: 0,
            bpc: 0,
            bda: 0,
            jumpdest: 0,
            dcic: 0,
            bad_vaddr: 0,
            bdam: 0,
            bpcm: 0,
            sr: 0x1090_0000,
            cause: 0,
            epc: 0,
            icache: vec![ICacheLine::default(); 256]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            gte: Gte::new(),
            load: (0, 0),
            is_branching: false,
            in_delay_slot: false,
        }
    }

    #[inline(always)]
    pub fn reg(&self, i: usize) -> u32 {
        self.regs[i]
    }

    #[inline(always)]
    pub fn set_reg(&mut self, i: usize, v: u32) {
        self.regs[i] = v;
        self.regs[0] = 0;
    }

    #[inline(always)]
    pub fn cache_isolated(&self) -> bool {
        (self.sr & 0x0001_0000) != 0
    }

    #[inline(always)]
    fn delayed_load(&mut self) {
        let (reg, val) = self.load;
        if reg != 0 {
            self.regs[reg] = val;
            self.regs[0] = 0;
            self.load = (0, 0);
        }
    }

    #[inline(always)]
    fn delayed_load_chain(&mut self, reg: usize, val: u32) {
        let (pending_reg, pending_val) = self.load;
        if pending_reg != 0 && pending_reg != reg {
            self.regs[pending_reg] = pending_val;
            self.regs[0] = 0;
        }
        self.load = (reg, val);
    }

    /// Busca a instrução passando pelo I-Cache de 4 KB quando em KUSEG/KSEG0
    fn fetch_instruction(&mut self, bus: &mut Bus) -> Instruction {
        let pc = self.current_pc;
        let cached_region = pc < 0xA000_0000;
        let icache_enabled = (bus.cache_ctrl & 0x0800) != 0;

        if cached_region && icache_enabled {
            let tag = pc & 0x7FFF_F000;
            let line_idx = ((pc >> 4) & 0xFF) as usize;
            let word_idx = ((pc >> 2) & 0x03) as usize;

            let line = &mut self.icache[line_idx];
            if line.tag() != tag || (line.valid_index() as usize) > word_idx {
                let mut fetch_pc = pc;
                for i in word_idx..4 {
                    line.words[i] = bus.read32(fetch_pc);
                    fetch_pc = fetch_pc.wrapping_add(4);
                }
                line.set_tag_valid(pc);
            }
            Instruction(line.words[word_idx])
        } else {
            Instruction(bus.read32(pc))
        }
    }

    /// Manutenção do Cache de Instruções quando SR bit 16 (Isolate Cache) está ativo
    fn cache_maintenance(&mut self, bus: &Bus, addr: u32, val: u32) {
        if (bus.cache_ctrl & 0x0800) == 0 {
            return;
        }
        let line_idx = ((addr >> 4) & 0xFF) as usize;
        let line = &mut self.icache[line_idx];

        if (bus.cache_ctrl & 0x04) != 0 {
            // Tag Test Mode: invalida a linha inteira
            line.invalidate();
        } else {
            let word_idx = ((addr >> 2) & 0x03) as usize;
            line.words[word_idx] = val;
        }
    }

    /// Executa apenas a instrução da CPU (sem dar tick individual nos periféricos)
    #[inline(always)]
    pub fn step_core(&mut self, bus: &mut Bus) {
        self.current_pc = self.pc;

        if (self.current_pc & 0x1FFF_FFFF) == 0x0000_00A0 {
            bus.bios.handle_tty(self.current_pc, &self.regs);
        }

        if self.current_pc % 4 != 0 {
            self.bad_vaddr = self.current_pc;
            self.delayed_load();
            self.trigger_exception(Exception::LoadAddressError);
            return;
        }

        let instr = self.fetch_instruction(bus);

        self.pc = self.next_pc;
        self.next_pc = self.pc.wrapping_add(4);

        self.in_delay_slot = self.is_branching;
        self.is_branching = false;

        let cause_val = (self.cause & !0x0400) | ((bus.irq.is_active() as u32) << 10);
        let irq_pending = (cause_val & self.sr) & 0x0700 != 0;

        if (self.sr & 1) != 0 && irq_pending {
            self.delayed_load();
            if instr.opcode() == 0b010010 && (instr.0 & (1 << 25)) != 0 {
                self.gte.execute(instr.0);
            }
            self.trigger_exception(Exception::Interrupt);
        } else {
            self.execute(instr, bus);
        }
    }

    /// Avança todos os periféricos de uma só vez por `cycles` ciclos de clock
    #[inline(always)]
    pub fn tick_peripherals(bus: &mut Bus, cycles: u32) {
        if crate::prof::enabled() {
            Self::tick_peripherals_profiled(bus, cycles);
            return;
        }
        bus.timers.tick(cycles, &mut bus.irq);
        bus.cdrom.tick(cycles, &mut bus.irq);
        if !bus.cdrom.audio_samples.is_empty() {
            for (l, r) in bus.cdrom.audio_samples.drain(..) {
                bus.spu.push_cd_audio(l, r);
            }
        }
        bus.controller.tick(cycles, &mut bus.irq);
        bus.spu.tick(cycles, &mut bus.irq);
    }

    #[inline(never)]
    fn tick_peripherals_profiled(bus: &mut Bus, cycles: u32) {
        use crate::prof::*;
        let t0 = now();
        bus.timers.tick(cycles, &mut bus.irq);
        let t1 = now();
        bus.cdrom.tick(cycles, &mut bus.irq);
        if !bus.cdrom.audio_samples.is_empty() {
            for (l, r) in bus.cdrom.audio_samples.drain(..) {
                bus.spu.push_cd_audio(l, r);
            }
        }
        let t2 = now();
        bus.controller.tick(cycles, &mut bus.irq);
        let t3 = now();
        bus.spu.tick(cycles, &mut bus.irq);
        let t4 = now();
        add(TIMERS, t1 - t0);
        add(CDROM, t2 - t1);
        add(PAD, t3 - t2);
        add(SPU, t4 - t3);
    }

    pub fn step(&mut self, bus: &mut Bus) {
        self.step_core(bus);
        Self::tick_peripherals(bus, 1);
    }

    #[inline(always)]
    fn branch(&mut self, offset: u32) {
        self.next_pc = self.pc.wrapping_add(offset << 2);
        self.is_branching = true;
    }

    pub fn trigger_exception(&mut self, cause: Exception) {
        let mode = self.sr & 0x3F;
        self.sr &= !0x3F;
        self.sr |= (mode << 2) & 0x3F;

        self.cause &= !0x7C;
        self.cause |= (cause as u32) << 2;

        if self.in_delay_slot {
            self.epc = self.current_pc.wrapping_sub(4);
            self.cause |= 1 << 31;
        } else {
            self.epc = self.current_pc;
            self.cause &= !(1 << 31);
        }

        self.pc = if (self.sr & (1 << 22)) != 0 {
            0xBFC0_0180
        } else {
            0x8000_0080
        };
        self.next_pc = self.pc.wrapping_add(4);
    }

    fn execute(&mut self, i: Instruction, bus: &mut Bus) {
        match i.opcode() {
            0b000000 => self.execute_special(i),
            0b000001 => self.execute_bxx(i),
            0b000010 => {
                self.next_pc = (self.pc & 0xF000_0000) | (i.imm_jump() << 2);
                self.is_branching = true;
                self.delayed_load();
            }
            0b000011 => {
                let ra = self.next_pc;
                self.next_pc = (self.pc & 0xF000_0000) | (i.imm_jump() << 2);
                self.delayed_load();
                self.set_reg(31, ra);
                self.is_branching = true;
            }
            0b000100 => {
                if self.reg(i.rs()) == self.reg(i.rt()) {
                    self.branch(i.imm_se());
                }
                self.delayed_load();
            }
            0b000101 => {
                if self.reg(i.rs()) != self.reg(i.rt()) {
                    self.branch(i.imm_se());
                }
                self.delayed_load();
            }
            0b000110 => {
                if (self.reg(i.rs()) as i32) <= 0 {
                    self.branch(i.imm_se());
                }
                self.delayed_load();
            }
            0b000111 => {
                if (self.reg(i.rs()) as i32) > 0 {
                    self.branch(i.imm_se());
                }
                self.delayed_load();
            }
            0b001000 => {
                let s = self.reg(i.rs()) as i32;
                let imm = i.imm_se() as i32;
                self.delayed_load();
                match s.checked_add(imm) {
                    Some(v) => self.set_reg(i.rt(), v as u32),
                    None => self.trigger_exception(Exception::Overflow),
                }
            }
            0b001001 => {
                let v = self.reg(i.rs()).wrapping_add(i.imm_se());
                self.delayed_load();
                self.set_reg(i.rt(), v);
            }
            0b001010 => {
                let v = ((self.reg(i.rs()) as i32) < (i.imm_se() as i32)) as u32;
                self.delayed_load();
                self.set_reg(i.rt(), v);
            }
            0b001011 => {
                let v = (self.reg(i.rs()) < i.imm_se()) as u32;
                self.delayed_load();
                self.set_reg(i.rt(), v);
            }
            0b001100 => {
                let v = self.reg(i.rs()) & i.imm();
                self.delayed_load();
                self.set_reg(i.rt(), v);
            }
            0b001101 => {
                let v = self.reg(i.rs()) | i.imm();
                self.delayed_load();
                self.set_reg(i.rt(), v);
            }
            0b001110 => {
                let v = self.reg(i.rs()) ^ i.imm();
                self.delayed_load();
                self.set_reg(i.rt(), v);
            }
            0b001111 => {
                let v = i.imm() << 16;
                self.delayed_load();
                self.set_reg(i.rt(), v);
            }
            0b010000 => self.execute_cop0(i, bus),
            0b010001 | 0b010011 => {
                self.delayed_load();
                self.trigger_exception(Exception::CoprocessorError);
            }
            0b010010 => self.execute_cop2(i),
            0b100000 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                let v = (bus.read8(addr) as i8) as i32 as u32;
                self.delayed_load_chain(i.rt(), v);
            }
            0b100001 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                if addr % 2 == 0 {
                    let v = (bus.read16(addr) as i16) as i32 as u32;
                    self.delayed_load_chain(i.rt(), v);
                } else {
                    self.bad_vaddr = addr;
                    self.delayed_load();
                    self.trigger_exception(Exception::LoadAddressError);
                }
            }
            0b100010 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                let (pending_reg, pending_val) = self.load;
                let cur = if pending_reg == i.rt() {
                    pending_val
                } else {
                    self.reg(i.rt())
                };
                let aligned = bus.read32(addr & !3);
                let v = match addr & 3 {
                    0 => (cur & 0x00FF_FFFF) | (aligned << 24),
                    1 => (cur & 0x0000_FFFF) | (aligned << 16),
                    2 => (cur & 0x0000_00FF) | (aligned << 8),
                    _ => aligned,
                };
                self.delayed_load_chain(i.rt(), v);
            }
            0b100011 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                if addr % 4 == 0 {
                    let v = bus.read32(addr);
                    self.delayed_load_chain(i.rt(), v);
                } else {
                    self.bad_vaddr = addr;
                    self.delayed_load();
                    self.trigger_exception(Exception::LoadAddressError);
                }
            }
            0b100100 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                let v = bus.read8(addr) as u32;
                self.delayed_load_chain(i.rt(), v);
            }
            0b100101 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                if addr % 2 == 0 {
                    let v = bus.read16(addr) as u32;
                    self.delayed_load_chain(i.rt(), v);
                } else {
                    self.bad_vaddr = addr;
                    self.delayed_load();
                    self.trigger_exception(Exception::LoadAddressError);
                }
            }
            0b100110 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                let (pending_reg, pending_val) = self.load;
                let cur = if pending_reg == i.rt() {
                    pending_val
                } else {
                    self.reg(i.rt())
                };
                let aligned = bus.read32(addr & !3);
                let v = match addr & 3 {
                    0 => aligned,
                    1 => (cur & 0xFF00_0000) | (aligned >> 8),
                    2 => (cur & 0xFFFF_0000) | (aligned >> 16),
                    _ => (cur & 0xFFFF_FF00) | (aligned >> 24),
                };
                self.delayed_load_chain(i.rt(), v);
            }
            0b101000 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                let v = self.reg(i.rt());
                self.delayed_load();
                if self.cache_isolated() {
                    self.cache_maintenance(bus, addr, v);
                } else {
                    bus.write8(addr, v as u8);
                }
            }
            0b101001 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                let v = self.reg(i.rt());
                self.delayed_load();
                if addr % 2 == 0 {
                    if self.cache_isolated() {
                        self.cache_maintenance(bus, addr, v);
                    } else {
                        bus.write16(addr, v as u16);
                    }
                } else {
                    self.bad_vaddr = addr;
                    self.trigger_exception(Exception::StoreAddressError);
                }
            }
            0b101010 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                let v = self.reg(i.rt());
                let cur = bus.read32(addr & !3);
                let merged = match addr & 3 {
                    0 => (cur & 0xFFFF_FF00) | (v >> 24),
                    1 => (cur & 0xFFFF_0000) | (v >> 16),
                    2 => (cur & 0xFF00_0000) | (v >> 8),
                    _ => v,
                };
                self.delayed_load();
                if self.cache_isolated() {
                    self.cache_maintenance(bus, addr & !3, merged);
                } else {
                    bus.write32(addr & !3, merged);
                }
            }
            0b101011 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                let v = self.reg(i.rt());
                self.delayed_load();
                if addr % 4 == 0 {
                    if self.cache_isolated() {
                        self.cache_maintenance(bus, addr, v);
                    } else {
                        bus.write32(addr, v);
                    }
                } else {
                    self.bad_vaddr = addr;
                    self.trigger_exception(Exception::StoreAddressError);
                }
            }
            0b101110 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                let v = self.reg(i.rt());
                let cur = bus.read32(addr & !3);
                let merged = match addr & 3 {
                    0 => v,
                    1 => (cur & 0x0000_00FF) | (v << 8),
                    2 => (cur & 0x0000_FFFF) | (v << 16),
                    _ => (cur & 0x00FF_FFFF) | (v << 24),
                };
                self.delayed_load();
                if self.cache_isolated() {
                    self.cache_maintenance(bus, addr & !3, merged);
                } else {
                    bus.write32(addr & !3, merged);
                }
            }
            0b110000 | 0b110001 | 0b110011 | 0b111000 | 0b111001 | 0b111011 => {
                self.delayed_load();
                self.trigger_exception(Exception::CoprocessorError);
            }
            0b110010 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                self.delayed_load();
                if addr % 4 == 0 {
                    let val = bus.read32(addr);
                    self.gte.write_data(i.rt(), val);
                } else {
                    self.bad_vaddr = addr;
                    self.trigger_exception(Exception::LoadAddressError);
                }
            }
            0b111010 => {
                let addr = self.reg(i.rs()).wrapping_add(i.imm_se());
                let val = self.gte.read_data(i.rt());
                self.delayed_load();
                if addr % 4 == 0 {
                    if !self.cache_isolated() {
                        bus.write32(addr, val);
                    }
                } else {
                    self.bad_vaddr = addr;
                    self.trigger_exception(Exception::StoreAddressError);
                }
            }
            _ => {
                self.delayed_load();
                self.trigger_exception(Exception::IllegalInstruction);
            }
        }
    }

    fn execute_special(&mut self, i: Instruction) {
        match i.subfunction() {
            0b000000 => {
                let v = self.reg(i.rt()) << i.shift();
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b000010 => {
                let v = self.reg(i.rt()) >> i.shift();
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b000011 => {
                let v = ((self.reg(i.rt()) as i32) >> i.shift()) as u32;
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b000100 => {
                let v = self.reg(i.rt()) << (self.reg(i.rs()) & 0x1F);
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b000110 => {
                let v = self.reg(i.rt()) >> (self.reg(i.rs()) & 0x1F);
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b000111 => {
                let v = ((self.reg(i.rt()) as i32) >> (self.reg(i.rs()) & 0x1F)) as u32;
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b001000 => {
                self.next_pc = self.reg(i.rs());
                self.delayed_load();
                self.is_branching = true;
            }
            0b001001 => {
                let ra = self.next_pc;
                self.next_pc = self.reg(i.rs());
                self.delayed_load();
                self.set_reg(i.rd(), ra);
                self.is_branching = true;
            }
            0b001100 => {
                self.delayed_load();
                self.trigger_exception(Exception::SysCall);
            }
            0b001101 => {
                self.delayed_load();
                self.trigger_exception(Exception::Break);
            }
            0b010000 => {
                let hi = self.hi;
                self.delayed_load();
                self.set_reg(i.rd(), hi);
            }
            0b010001 => {
                self.hi = self.reg(i.rs());
                self.delayed_load();
            }
            0b010010 => {
                let lo = self.lo;
                self.delayed_load();
                self.set_reg(i.rd(), lo);
            }
            0b010011 => {
                self.lo = self.reg(i.rs());
                self.delayed_load();
            }
            0b011000 => {
                let a = (self.reg(i.rs()) as i32) as i64;
                let b = (self.reg(i.rt()) as i32) as i64;
                self.delayed_load();
                let res = (a * b) as u64;
                self.hi = (res >> 32) as u32;
                self.lo = res as u32;
            }
            0b011001 => {
                let res = (self.reg(i.rs()) as u64) * (self.reg(i.rt()) as u64);
                self.delayed_load();
                self.hi = (res >> 32) as u32;
                self.lo = res as u32;
            }
            0b011010 => {
                let n = self.reg(i.rs()) as i32;
                let d = self.reg(i.rt()) as i32;
                self.delayed_load();
                if d == 0 {
                    self.hi = n as u32;
                    self.lo = if n >= 0 { 0xFFFF_FFFF } else { 1 };
                } else if n as u32 == 0x8000_0000 && d == -1 {
                    self.hi = 0;
                    self.lo = 0x8000_0000;
                } else {
                    self.hi = (n % d) as u32;
                    self.lo = (n / d) as u32;
                }
            }
            0b011011 => {
                let n = self.reg(i.rs());
                let d = self.reg(i.rt());
                self.delayed_load();
                if d == 0 {
                    self.hi = n;
                    self.lo = 0xFFFF_FFFF;
                } else {
                    self.hi = n % d;
                    self.lo = n / d;
                }
            }
            0b100000 => {
                let s = self.reg(i.rs()) as i32;
                let t = self.reg(i.rt()) as i32;
                self.delayed_load();
                match s.checked_add(t) {
                    Some(v) => self.set_reg(i.rd(), v as u32),
                    None => self.trigger_exception(Exception::Overflow),
                }
            }
            0b100001 => {
                let v = self.reg(i.rs()).wrapping_add(self.reg(i.rt()));
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b100010 => {
                let s = self.reg(i.rs()) as i32;
                let t = self.reg(i.rt()) as i32;
                self.delayed_load();
                match s.checked_sub(t) {
                    Some(v) => self.set_reg(i.rd(), v as u32),
                    None => self.trigger_exception(Exception::Overflow),
                }
            }
            0b100011 => {
                let v = self.reg(i.rs()).wrapping_sub(self.reg(i.rt()));
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b100100 => {
                let v = self.reg(i.rs()) & self.reg(i.rt());
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b100101 => {
                let v = self.reg(i.rs()) | self.reg(i.rt());
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b100110 => {
                let v = self.reg(i.rs()) ^ self.reg(i.rt());
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b100111 => {
                let v = !(self.reg(i.rs()) | self.reg(i.rt()));
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b101010 => {
                let v = ((self.reg(i.rs()) as i32) < (self.reg(i.rt()) as i32)) as u32;
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            0b101011 => {
                let v = (self.reg(i.rs()) < self.reg(i.rt())) as u32;
                self.delayed_load();
                self.set_reg(i.rd(), v);
            }
            _ => {
                self.delayed_load();
                self.trigger_exception(Exception::IllegalInstruction);
            }
        }
    }

    fn execute_bxx(&mut self, i: Instruction) {
        let is_bgez = (i.0 >> 16) & 1 != 0;
        let is_link = (i.0 >> 17) & 0xF == 0x8;
        let v = self.reg(i.rs()) as i32;
        let test = if is_bgez { v >= 0 } else { v < 0 };

        let ra = self.next_pc;
        self.delayed_load();

        if is_link {
            self.set_reg(31, ra);
        }
        if test {
            self.branch(i.imm_se());
        }
    }

    fn execute_cop0(&mut self, i: Instruction, bus: &mut Bus) {
        match i.cop_opcode() {
            0b00000 => {
                let val = match i.rd() {
                    3 => self.bpc,
                    5 => self.bda,
                    6 => self.jumpdest,
                    7 => self.dcic,
                    8 => self.bad_vaddr,
                    9 => self.bdam,
                    11 => self.bpcm,
                    12 => self.sr,
                    13 => (self.cause & !0x0400) | ((bus.irq.is_active() as u32) << 10),
                    14 => self.epc,
                    15 => 0x0000_0002,
                    _ => 0,
                };
                self.delayed_load_chain(i.rt(), val);
            }
            0b00100 => {
                let val = self.reg(i.rt());
                self.delayed_load();
                match i.rd() {
                    3 => self.bpc = val,
                    5 => self.bda = val,
                    6 => self.jumpdest = val,
                    7 => self.dcic = val,
                    8 => self.bad_vaddr = val,
                    9 => self.bdam = val,
                    11 => self.bpcm = val,
                    12 => self.sr = val,
                    13 => {
                        self.cause &= !0x0300;
                        self.cause |= val & 0x0300;
                    }
                    14 => self.epc = val,
                    _ => {}
                }
            }
            0b10000 => {
                self.delayed_load();
                let mode = self.sr & 0x3F;
                self.sr &= !0x0F;
                self.sr |= mode >> 2;
            }
            _ => {
                self.delayed_load();
                self.trigger_exception(Exception::IllegalInstruction);
            }
        }
    }

    fn execute_cop2(&mut self, i: Instruction) {
        if (i.0 & (1 << 25)) != 0 {
            self.delayed_load();
            self.gte.execute(i.0);
        } else {
            match i.cop_opcode() {
                0b00000 => {
                    let v = self.gte.read_data(i.rd());
                    self.delayed_load_chain(i.rt(), v);
                }
                0b00010 => {
                    let v = self.gte.read_ctrl(i.rd());
                    self.delayed_load_chain(i.rt(), v);
                }
                0b00100 => {
                    let v = self.reg(i.rt());
                    self.delayed_load();
                    self.gte.write_data(i.rd(), v);
                }
                0b00110 => {
                    let v = self.reg(i.rt());
                    self.delayed_load();
                    self.gte.write_ctrl(i.rd(), v);
                }
                _ => {
                    self.delayed_load();
                    self.trigger_exception(Exception::IllegalInstruction);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_addu_and_subu() {
        let mut bus = Bus::new_empty();
        let mut cpu = Cpu::new();

        cpu.set_reg(1, 10);
        cpu.set_reg(2, 25);

        bus.write32(0x0000_0000, 0x00221821);
        cpu.pc = 0x8000_0000;
        cpu.next_pc = 0x8000_0004;
        cpu.step(&mut bus);
        assert_eq!(cpu.reg(3), 35);

        bus.write32(0x0000_0004, 0x00412023);
        cpu.step(&mut bus);
        assert_eq!(cpu.reg(4), 15);
    }

    #[test]
    fn test_branch_delay_slot() {
        let mut bus = Bus::new_empty();
        let mut cpu = Cpu::new();
        cpu.pc = 0x8000_0000;
        cpu.next_pc = 0x8000_0004;

        bus.write32(0x0000_0000, 0x1000_0002);
        bus.write32(0x0000_0004, 0x2401_002A);
        bus.write32(0x0000_0008, 0x2402_0063);

        cpu.step(&mut bus);
        assert_eq!(cpu.reg(1), 0);

        cpu.step(&mut bus);
        assert_eq!(cpu.reg(1), 42);
        assert_eq!(cpu.pc, 0x8000_000C);
        assert_eq!(cpu.reg(2), 0);
    }

    #[test]
    fn test_load_delay_slot() {
        let mut bus = Bus::new_empty();
        let mut cpu = Cpu::new();
        cpu.pc = 0x8000_0000;
        cpu.next_pc = 0x8000_0004;

        bus.write32(0x0000_1000, 0x1234_5678);
        cpu.set_reg(1, 0x0000_1000);

        bus.write32(0x0000_0000, 0x8C22_0000);
        bus.write32(0x0000_0004, 0x2443_0000);

        cpu.step(&mut bus);
        assert_eq!(cpu.reg(2), 0);

        cpu.step(&mut bus);
        assert_eq!(cpu.reg(3), 0);
        assert_eq!(cpu.reg(2), 0x1234_5678);
    }

    #[test]
    fn test_unaligned_lwl_lwr() {
        let mut bus = Bus::new_empty();
        let mut cpu = Cpu::new();
        cpu.pc = 0x8000_0000;
        cpu.next_pc = 0x8000_0004;

        bus.write32(0x0000_1000, 0x7654_3210);
        bus.write32(0x0000_1004, 0xFEDC_BA98);
        cpu.set_reg(1, 0x0000_1000);

        bus.write32(0x0000_0000, 0x9822_0001);
        bus.write32(0x0000_0004, 0x8822_0004);

        cpu.step(&mut bus);
        cpu.step(&mut bus);
        cpu.step(&mut bus);

        assert_eq!(cpu.reg(2), 0x9876_5432);
    }
}
