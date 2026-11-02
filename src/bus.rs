// src/bus.rs

use crate::bios::Bios;
#[cfg(test)]
use crate::bios::BIOS_SIZE;
use crate::cdrom::CdRom;
use crate::controller::Controller;
use crate::dma::{Dma, DmaPort};
use crate::gpu::Gpu;
use crate::interrupt::Interrupts;
use crate::mdec::Mdec;
use crate::spu::Spu;
use crate::timers::Timers;

pub const RAM_SIZE: usize = 2 * 1024 * 1024;
pub const SCRATCHPAD_SIZE: usize = 1024;

pub struct Bus {
    pub ram: Box<[u8; RAM_SIZE]>,
    pub bios: Bios,
    pub scratchpad: Box<[u8; SCRATCHPAD_SIZE]>,
    pub irq: Interrupts,
    pub timers: Timers,
    pub dma: Dma,
    pub gpu: Gpu,
    pub cdrom: CdRom,
    pub controller: Controller,
    pub spu: Spu,
    pub mdec: Mdec,
    pub mem_ctrl: [u32; 9],
    pub ram_size_reg: u32,
    pub cache_ctrl: u32,
}

impl Bus {
    pub fn new(bios_data: Vec<u8>) -> Self {
        let bios = Bios::new(bios_data).expect("Erro ao carregar arquivo de BIOS!");
        Self {
            ram: vec![0x00; RAM_SIZE].into_boxed_slice().try_into().unwrap(),
            bios,
            scratchpad: vec![0u8; SCRATCHPAD_SIZE]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            irq: Interrupts::new(),
            timers: Timers::new(),
            dma: Dma::new(),
            gpu: Gpu::new(),
            cdrom: CdRom::new(),
            controller: Controller::new(),
            spu: Spu::new(),
            mdec: Mdec::new(),
            mem_ctrl: [0; 9],
            ram_size_reg: 0x0000_0B88,
            cache_ctrl: 0,
        }
    }

    #[cfg(test)]
    pub fn new_empty() -> Self {
        Self::new(vec![0; BIOS_SIZE])
    }

    #[inline(always)]
    pub fn physical_address(addr: u32) -> u32 {
        const REGION_MASKS: [u32; 8] = [
            0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF,
            0x7FFF_FFFF,
            0x1FFF_FFFF,
            0xFFFF_FFFF, 0xFFFF_FFFF,
        ];
        addr & REGION_MASKS[(addr >> 29) as usize]
    }

    // =========================================================================
    // Leituras (8, 16 e 32 bits)
    // =========================================================================

    pub fn read8(&mut self, addr: u32) -> u8 {
        let phys = Self::physical_address(addr);

        if phys < 0x0080_0000 {
            return self.ram[(phys & 0x001F_FFFF) as usize];
        }

        if (0x1F80_0000..=0x1F80_03FF).contains(&phys) {
            return self.scratchpad[(phys & 0x3FF) as usize];
        }

        if (0x1FC0_0000..=0x1FC7_FFFF).contains(&phys) {
            return self.bios.read8((phys - 0x1FC0_0000) as usize);
        }

        if (0x1F80_1040..=0x1F80_104F).contains(&phys) {
            return match phys - 0x1F80_1040 {
                0 => self.controller.read_data(),
                4..=7 => (self.controller.read_status() >> ((phys & 3) * 8)) as u8,
                8..=9 => (self.controller.mode >> ((phys & 1) * 8)) as u8,
                10..=11 => (self.controller.ctrl >> ((phys & 1) * 8)) as u8,
                14..=15 => (self.controller.baud >> ((phys & 1) * 8)) as u8,
                _ => 0xFF,
            };
        }

        if (0x1F80_1800..=0x1F80_1803).contains(&phys) {
            return self.cdrom.read(phys - 0x1F80_1800);
        }

        if (0x1F80_1080..=0x1F80_10FF).contains(&phys) {
            let align = phys & 3;
            let word = self.dma.read((phys & !3) - 0x1F80_1080);
            return (word >> (align * 8)) as u8;
        }

        if phys == 0x1F80_1070 || phys == 0x1F80_1071 {
            return (self.irq.status >> ((phys & 1) * 8)) as u8;
        }

        if phys == 0x1F80_1074 || phys == 0x1F80_1075 {
            return (self.irq.mask >> ((phys & 1) * 8)) as u8;
        }

        if (0x1F00_0000..=0x1F07_FFFF).contains(&phys) {
            return 0xFF;
        }

        if phys == 0x1F80_2021 {
            return 1 << 2;
        }

        0
    }

    pub fn read16(&mut self, addr: u32) -> u16 {
        let phys = Self::physical_address(addr);

        if phys < 0x0080_0000 {
            let offset = (phys & 0x001F_FFFE) as usize;
            return u16::from_le(unsafe { std::ptr::read_unaligned(self.ram.as_ptr().add(offset) as *const u16) });
        }

        if (0x1F80_0000..=0x1F80_03FF).contains(&phys) {
            let offset = (phys & 0x3FF) as usize;
            return u16::from_le_bytes(self.scratchpad[offset..offset + 2].try_into().unwrap());
        }

        if (0x1FC0_0000..=0x1FC7_FFFF).contains(&phys) {
            return self.bios.read16((phys - 0x1FC0_0000) as usize);
        }

        if phys == 0x1F80_1040 { return self.controller.read_data() as u16; }
        if phys == 0x1F80_1044 { return self.controller.read_status() as u16; }
        if phys == 0x1F80_1048 { return self.controller.mode; }
        if phys == 0x1F80_104A { return self.controller.ctrl; }
        if phys == 0x1F80_104E { return self.controller.baud; }

        if phys == 0x1F80_1070 { return self.irq.status; }
        if phys == 0x1F80_1074 { return self.irq.mask; }

        if (0x1F80_1100..=0x1F80_1128).contains(&phys) {
            return self.timers.read(phys - 0x1F80_1100) as u16;
        }

        if (0x1F80_1C00..=0x1F80_1E7F).contains(&phys) {
            let offset = phys - 0x1F80_1C00;
            if offset == 0x01A8 {
                return self.spu.read_fifo(&mut self.irq);
            }
            return self.spu.read(offset);
        }

        if (0x1F80_1080..=0x1F80_10FF).contains(&phys) {
            let align = phys & 2;
            let word = self.dma.read((phys & !3) - 0x1F80_1080);
            return (word >> (align * 8)) as u16;
        }

        let b0 = self.read8(addr) as u16;
        let b1 = self.read8(addr.wrapping_add(1)) as u16;
        b0 | (b1 << 8)
    }

    pub fn read32(&mut self, addr: u32) -> u32 {
        let phys = Self::physical_address(addr);

        if phys < 0x0080_0000 {
            let offset = (phys & 0x001F_FFFC) as usize;
            return u32::from_le(unsafe { std::ptr::read_unaligned(self.ram.as_ptr().add(offset) as *const u32) });
        }

        if (0x1F80_0000..=0x1F80_03FF).contains(&phys) {
            let offset = (phys & 0x3FF) as usize;
            return u32::from_le_bytes(self.scratchpad[offset..offset + 4].try_into().unwrap());
        }

        if (0x1FC0_0000..=0x1FC7_FFFF).contains(&phys) {
            return self.bios.read32((phys - 0x1FC0_0000) as usize);
        }

        if (0x1F80_1000..=0x1F80_1020).contains(&phys) {
            let idx = ((phys - 0x1F80_1000) >> 2) as usize;
            return self.mem_ctrl[idx];
        }

        if phys == 0x1F80_1040 { return self.controller.read_data() as u32; }
        if phys == 0x1F80_1044 { return self.controller.read_status(); }
        if phys == 0x1F80_1048 { return self.controller.mode as u32; }
        if phys == 0x1F80_104A { return self.controller.ctrl as u32; }
        if phys == 0x1F80_104E { return self.controller.baud as u32; }

        if phys == 0x1F80_1060 { return self.ram_size_reg; }
        if phys == 0x1F80_1070 { return self.irq.status as u32; }
        if phys == 0x1F80_1074 { return self.irq.mask as u32; }

        if (0x1F80_1080..=0x1F80_10FF).contains(&phys) {
            return self.dma.read(phys - 0x1F80_1080);
        }

        if (0x1F80_1100..=0x1F80_1128).contains(&phys) {
            return self.timers.read(phys - 0x1F80_1100);
        }

        if phys == 0x1F80_1810 { return self.gpu.read_data(); }
        if phys == 0x1F80_1814 { return self.gpu.read_status(); }

        if phys == 0x1F80_1820 { return self.mdec.read_data(); }
        if phys == 0x1F80_1824 { return self.mdec.read_status(); }

        if (0x1F80_1C00..=0x1F80_1E7F).contains(&phys) {
            let offset = phys - 0x1F80_1C00;
            if offset == 0x01A8 {
                let p0 = self.spu.read_fifo(&mut self.irq) as u32;
                let p1 = self.spu.read_fifo(&mut self.irq) as u32;
                return p0 | (p1 << 16);
            }
            let low = self.spu.read(offset) as u32;
            let high = self.spu.read(offset + 2) as u32;
            return low | (high << 16);
        }

        if phys == 0xFFFE_0130 { return self.cache_ctrl; }

        0
    }

    // =========================================================================
    // Escritas (8, 16 e 32 bits)
    // =========================================================================

    pub fn write8(&mut self, addr: u32, val: u8) {
        let phys = Self::physical_address(addr);

        if phys < 0x0080_0000 {
            self.ram[(phys & 0x001F_FFFF) as usize] = val;
            return;
        }

        if (0x1F80_0000..=0x1F80_03FF).contains(&phys) {
            self.scratchpad[(phys & 0x3FF) as usize] = val;
            return;
        }

        if (0x1F80_1040..=0x1F80_104F).contains(&phys) {
            match phys - 0x1F80_1040 {
                0 => self.controller.write_data(val, &mut self.irq),
                8 => self.controller.mode = (self.controller.mode & 0xFF00) | (val as u16),
                9 => self.controller.mode = (self.controller.mode & 0x00FF) | ((val as u16) << 8),
                10 => {
                    let new_ctrl = (self.controller.ctrl & 0xFF00) | (val as u16);
                    self.controller.write_ctrl(new_ctrl, &mut self.irq);
                }
                11 => {
                    let new_ctrl = (self.controller.ctrl & 0x00FF) | ((val as u16) << 8);
                    self.controller.write_ctrl(new_ctrl, &mut self.irq);
                }
                14 => self.controller.baud = (self.controller.baud & 0xFF00) | (val as u16),
                15 => self.controller.baud = (self.controller.baud & 0x00FF) | ((val as u16) << 8),
                _ => {}
            }
            return;
        }

        if (0x1F80_1800..=0x1F80_1803).contains(&phys) {
            self.cdrom.write(phys - 0x1F80_1800, val, &mut self.irq);
            return;
        }

        if (0x1F80_1080..=0x1F80_10FF).contains(&phys) {
            let offset = phys - 0x1F80_1080;
            self.dma.write8(offset, val, &mut self.irq);
            self.check_and_run_dma(offset);
            return;
        }
    }

    pub fn write16(&mut self, addr: u32, val: u16) {
        let phys = Self::physical_address(addr);

        if phys < 0x0080_0000 {
            let offset = (phys & 0x001F_FFFE) as usize;
            unsafe { std::ptr::write_unaligned(self.ram.as_mut_ptr().add(offset) as *mut u16, val.to_le()); }
            return;
        }

        if (0x1F80_0000..=0x1F80_03FF).contains(&phys) {
            let offset = (phys & 0x3FF) as usize;
            self.scratchpad[offset..offset + 2].copy_from_slice(&val.to_le_bytes());
            return;
        }

        if phys == 0x1F80_1040 { self.controller.write_data(val as u8, &mut self.irq); return; }
        if phys == 0x1F80_1048 { self.controller.mode = val; return; }
        if phys == 0x1F80_104A { self.controller.write_ctrl(val, &mut self.irq); return; }
        if phys == 0x1F80_104E { self.controller.baud = val; return; }

        if phys == 0x1F80_1070 { self.irq.acknowledge(val); return; }
        if phys == 0x1F80_1074 { self.irq.mask = val; return; }

        if (0x1F80_1100..=0x1F80_1128).contains(&phys) {
            self.timers.write(phys - 0x1F80_1100, val as u32);
            return;
        }

        if (0x1F80_1C00..=0x1F80_1E7F).contains(&phys) {
            let offset = phys - 0x1F80_1C00;
            if offset == 0x01A8 {
                self.spu.write_fifo(val, &mut self.irq);
                return;
            }
            self.spu.write(offset, val);
            return;
        }

        if (0x1F80_1080..=0x1F80_10FF).contains(&phys) {
            let offset = phys - 0x1F80_1080;
            self.dma.write16(offset, val, &mut self.irq);
            self.check_and_run_dma(offset);
            return;
        }
    }

    pub fn write32(&mut self, addr: u32, val: u32) {
        let phys = Self::physical_address(addr);

        if phys < 0x0080_0000 {
            let offset = (phys & 0x001F_FFFC) as usize;
            unsafe { std::ptr::write_unaligned(self.ram.as_mut_ptr().add(offset) as *mut u32, val.to_le()); }
            return;
        }

        if (0x1F80_0000..=0x1F80_03FF).contains(&phys) {
            let offset = (phys & 0x3FF) as usize;
            self.scratchpad[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
            return;
        }

        if (0x1F80_1000..=0x1F80_1020).contains(&phys) {
            let idx = ((phys - 0x1F80_1000) >> 2) as usize;
            self.mem_ctrl[idx] = val;
            return;
        }

        if phys == 0x1F80_1040 { self.controller.write_data(val as u8, &mut self.irq); return; }
        if phys == 0x1F80_1048 { self.controller.mode = val as u16; return; }
        if phys == 0x1F80_104A { self.controller.write_ctrl(val as u16, &mut self.irq); return; }
        if phys == 0x1F80_104E { self.controller.baud = val as u16; return; }

        if phys == 0x1F80_1060 { self.ram_size_reg = val; return; }
        if phys == 0x1F80_1070 { self.irq.acknowledge(val as u16); return; }
        if phys == 0x1F80_1074 { self.irq.mask = val as u16; return; }

        if (0x1F80_1080..=0x1F80_10FF).contains(&phys) {
            let offset = phys - 0x1F80_1080;
            self.dma.write(offset, val, &mut self.irq);
            self.check_and_run_dma(offset);
            return;
        }

        if (0x1F80_1100..=0x1F80_1128).contains(&phys) {
            self.timers.write(phys - 0x1F80_1100, val);
            return;
        }

        if (0x1F80_1800..=0x1F80_1803).contains(&phys) {
            self.cdrom.write(phys - 0x1F80_1800, val as u8, &mut self.irq);
            return;
        }

        if phys == 0x1F80_1810 { self.gpu.write_gp0(val); return; }
        if phys == 0x1F80_1814 { self.gpu.write_gp1(val); return; }

        if phys == 0x1F80_1820 { self.mdec.write_command(val); return; }
        if phys == 0x1F80_1824 { self.mdec.write_control(val); return; }

        if (0x1F80_1C00..=0x1F80_1E7F).contains(&phys) {
            let offset = phys - 0x1F80_1C00;
            if offset == 0x01A8 {
                self.spu.write_fifo(val as u16, &mut self.irq);
                self.spu.write_fifo((val >> 16) as u16, &mut self.irq);
                return;
            }
            self.spu.write(offset, val as u16);
            self.spu.write(offset + 2, (val >> 16) as u16);
            return;
        }

        if phys == 0xFFFE_0130 {
            self.cache_ctrl = val;
        }
    }

    fn check_and_run_dma(&mut self, offset: u32) {
        let aligned = offset & !3;
        let channel_idx = (aligned >> 4) as usize;
        if (aligned & 0xF) == 0x8 && channel_idx < 7 {
            if self.dma.channels[channel_idx].is_active() {
                let port = match channel_idx {
                    0 => DmaPort::MdecIn,
                    1 => DmaPort::MdecOut,
                    2 => DmaPort::Gpu,
                    3 => DmaPort::CdRom,
                    4 => DmaPort::Spu,
                    5 => DmaPort::Pio,
                    _ => DmaPort::Otc,
                };
                self.execute_dma(port);
            }
        }
    }

    // =========================================================================
    // Executor de Transferências DMA (Modos 0, 1 e 2)
    // =========================================================================

    pub fn execute_dma(&mut self, port: DmaPort) {
        if crate::prof::enabled() {
            let t0 = crate::prof::now();
            self.execute_dma_inner(port);
            crate::prof::add(crate::prof::DMA, crate::prof::now() - t0);
        } else {
            self.execute_dma_inner(port);
        }
    }

    fn execute_dma_inner(&mut self, port: DmaPort) {
        
        let ch = self.dma.channels[port as usize];

        match ch.sync_mode() {
            0 | 1 => {
                let step = if ch.is_decrement() { -4i32 as u32 } else { 4u32 };
                let mut addr = ch.base_addr;

                let mut remsz = if ch.sync_mode() == 0 {
                    let sz = ch.block_ctrl & 0xFFFF;
                    if sz == 0 { 0x10000 } else { sz }
                } else {
                    let block_size = ch.block_ctrl & 0xFFFF;
                    let block_count = (ch.block_ctrl >> 16) & 0xFFFF;
                    block_size * block_count
                };

                while remsz > 0 {
                    let cur_addr = addr & 0x001F_FFFC;

                    if ch.is_from_ram() {
                        let src_word = u32::from_le_bytes(
                            self.ram[cur_addr as usize..cur_addr as usize + 4]
                                .try_into()
                                .unwrap(),
                        );

                        if port == DmaPort::MdecIn || port == DmaPort::MdecOut || port == DmaPort::Gpu {
            
        }
        match port {
                            DmaPort::Gpu => self.gpu.write_gp0(src_word),
                            DmaPort::MdecIn => self.mdec.write_command(src_word),
                            DmaPort::Spu => {
                                self.spu.write_fifo(src_word as u16, &mut self.irq);
                                self.spu.write_fifo((src_word >> 16) as u16, &mut self.irq);
                            }
                            _ => {}
                        }
                    } else {
                        let src_word = match port {
                            DmaPort::Otc => match remsz {
                                1 => 0x00FF_FFFF,
                                _ => addr.wrapping_sub(4) & 0x001F_FFFF,
                            },
                            DmaPort::Gpu => self.gpu.read_data(),
                            DmaPort::CdRom => self.cdrom.dma_read_word(),
                            DmaPort::MdecOut => self.mdec.read_data(),
                            DmaPort::Spu => {
                                let p0 = self.spu.read_fifo(&mut self.irq) as u32;
                                let p1 = self.spu.read_fifo(&mut self.irq) as u32;
                                p0 | (p1 << 16)
                            }
                            _ => 0,
                        };

                        self.ram[cur_addr as usize..cur_addr as usize + 4]
                            .copy_from_slice(&src_word.to_le_bytes());
                    }

                    addr = addr.wrapping_add(step);
                    remsz -= 1;
                }
            }
            2 => {
                let mut addr = ch.base_addr & 0x001F_FFFC;
                let mut max_nodes = 0x10000; // Proteção contra listas circulares infinitas

                while max_nodes > 0 {
                    let offset = (addr & 0x001F_FFFC) as usize;
                    let header = u32::from_le(unsafe { std::ptr::read_unaligned(self.ram.as_ptr().add(offset) as *const u32) });
                    let mut rem_words = header >> 24;

                    while rem_words > 0 {
                        addr = (addr + 4) & 0x001F_FFFC;
                        let cmd_offset = addr as usize;
                        let cmd = u32::from_le(unsafe { std::ptr::read_unaligned(self.ram.as_ptr().add(cmd_offset) as *const u32) });
                        self.gpu.write_gp0(cmd);
                        rem_words -= 1;
                    }

                    self.dma.channels[port as usize].base_addr = header & 0x00FF_FFFF;

                    if (header & 0x0080_0000) != 0 || (header & 0x00FF_FFFF) == 0x00FF_FFFF {
                        break;
                    }

                    addr = header & 0x001F_FFFC;
                    max_nodes -= 1;
                }
            }
            _ => {}
        }

        self.dma.trigger_done(port, &mut self.irq);
    }
}

// =============================================================================
// Testes Unitários do Barramento e Integrações DMA
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ram_read_write_endianness() {
        let mut bus = Bus::new_empty();
        bus.write32(0x0000_1000, 0x1234_5678);

        assert_eq!(bus.read32(0x0000_1000), 0x1234_5678);
        assert_eq!(bus.read16(0x0000_1000), 0x5678);
        assert_eq!(bus.read16(0x0000_1002), 0x1234);
        assert_eq!(bus.read8(0x0000_1000), 0x78);
        assert_eq!(bus.read8(0x0000_1001), 0x56);
        assert_eq!(bus.read8(0x0000_1002), 0x34);
        assert_eq!(bus.read8(0x0000_1003), 0x12);
    }

    #[test]
    fn test_ram_mirroring_and_regions() {
        let mut bus = Bus::new_empty();
        bus.write32(0x0000_0000, 0xDEAD_BEEF);

        assert_eq!(bus.read32(0x8000_0000), 0xDEAD_BEEF);
        assert_eq!(bus.read32(0xA000_0000), 0xDEAD_BEEF);
        assert_eq!(bus.read32(0x0020_0000), 0xDEAD_BEEF);
    }

    #[test]
    fn test_scratchpad() {
        let mut bus = Bus::new_empty();
        bus.write32(0x1F80_0000, 0xCAFE_BABE);
        assert_eq!(bus.read32(0x1F80_0000), 0xCAFE_BABE);
        assert_eq!(bus.read16(0x1F80_0000), 0xBABE);
    }

    #[test]
    fn test_bios_mapping() {
        let mut bios_dummy = vec![0u8; BIOS_SIZE];
        bios_dummy[0] = 0x34;
        bios_dummy[1] = 0x12;
        bios_dummy[2] = 0x08;
        bios_dummy[3] = 0x3C;

        let mut bus = Bus::new(bios_dummy);
        assert_eq!(bus.read32(0xBFC0_0000), 0x3C08_1234);
    }

    #[test]
    fn test_dma_otc_clearing() {
        let mut bus = Bus::new_empty();

        bus.write32(0x1F80_10E0, 0x0000_100C);
        bus.write32(0x1F80_10E4, 4);
        bus.write32(0x1F80_10E8, (1 << 24) | (1 << 28) | (1 << 1));

        assert_eq!(bus.read32(0x0000_100C), 0x0000_1008);
        assert_eq!(bus.read32(0x0000_1008), 0x0000_1004);
        assert_eq!(bus.read32(0x0000_1004), 0x0000_1000);
        assert_eq!(bus.read32(0x0000_1000), 0x00FF_FFFF);
    }

    #[test]
    fn test_dma_linked_list_to_gpu() {
        let mut bus = Bus::new_empty();

        bus.write32(0x0000_1000, 0x03FF_FFFF);
        bus.write32(0x0000_1004, 0x02FF_0000);
        bus.write32(0x0000_1008, 0x0000_0000);
        bus.write32(0x0000_100C, 0x0010_0010);

        bus.write32(0x1F80_10A0, 0x0000_1000);
        bus.write32(0x1F80_10A8, (1 << 24) | (2 << 9) | 1);

        assert_eq!(bus.gpu.read_vram(0, 0), 0x7C00);
        assert_eq!(bus.gpu.read_vram(15, 15), 0x7C00);
    }

    #[test]
    fn test_dma_cdrom_to_ram() {
        let mut bus = Bus::new_empty();

        bus.cdrom.sector_read_index = 0; // Começa do índice 0 para o teste unitário
        bus.cdrom.sector_buffer[0] = 0xAA;
        bus.cdrom.sector_buffer[1] = 0xBB;
        bus.cdrom.sector_buffer[2] = 0xCC;
        bus.cdrom.sector_buffer[3] = 0xDD;

        bus.write32(0x1F80_10B0, 0x0000_2000);
        bus.write32(0x1F80_10B4, 1);
        bus.write32(0x1F80_10B8, (1 << 24) | (1 << 28));

        assert_eq!(bus.read32(0x0000_2000), 0xDDCC_BBAA);
    }

    #[test]
    fn test_dma_ram_to_spu() {
        let mut bus = Bus::new_empty();
        bus.spu.transfer_addr = 0x0800;
        bus.write32(0x0000_3000, 0xCAFE_BABE);

        bus.write32(0x1F80_10C0, 0x0000_3000);
        bus.write32(0x1F80_10C4, 1);
        bus.write32(0x1F80_10C8, (1 << 24) | (1 << 28) | 1);

        assert_eq!(bus.spu.ram[0x0800], 0xBABE);
        assert_eq!(bus.spu.ram[0x0801], 0xCAFE);
    }

    #[test]
    fn test_dma_ram_to_mdec() {
        let mut bus = Bus::new_empty();
        bus.write32(0x1F80_1820, 0x4000_0000);
        bus.write32(0x0000_4000, 0x0807_0605);

        bus.write32(0x1F80_1080, 0x0000_4000);
        bus.write32(0x1F80_1084, 1);
        bus.write32(0x1F80_1088, (1 << 24) | (1 << 28) | 1);

        assert_eq!(bus.mdec.luma_quant[0], 0x05);
        assert_eq!(bus.mdec.luma_quant[1], 0x06);
        assert_eq!(bus.mdec.luma_quant[2], 0x07);
        assert_eq!(bus.mdec.luma_quant[3], 0x08);
    }
}
