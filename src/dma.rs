// src/dma.rs

use crate::interrupt::{InterruptType, Interrupts};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum DmaPort {
    MdecIn = 0,
    MdecOut = 1,
    Gpu = 2,
    CdRom = 3,
    Spu = 4,
    Pio = 5,
    Otc = 6,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Channel {
    pub base_addr: u32,
    pub block_ctrl: u32,
    pub ctrl: u32,
}

impl Channel {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        let enable = (self.ctrl & (1 << 24)) != 0;
        let sync_mode = (self.ctrl >> 9) & 3;
        let trigger = if sync_mode == 0 {
            (self.ctrl & (1 << 28)) != 0
        } else {
            true
        };
        enable && trigger
    }

    #[inline(always)]
    pub fn is_from_ram(&self) -> bool {
        (self.ctrl & 1) != 0
    }

    #[inline(always)]
    pub fn is_decrement(&self) -> bool {
        (self.ctrl & (1 << 1)) != 0
    }

    #[inline(always)]
    pub fn sync_mode(&self) -> u32 {
        (self.ctrl >> 9) & 3
    }

    pub fn finish(&mut self) {
        self.ctrl &= !(1 << 24);
        self.ctrl &= !(1 << 28);
    }
}

pub struct Dma {
    pub control: u32,
    pub interrupt: u32,
    pub channels: [Channel; 7],
}

impl Dma {
    pub fn new() -> Self {
        Self {
            control: 0x0765_4321,
            interrupt: 0,
            channels: [Channel::new(); 7],
        }
    }

    pub fn read(&self, offset: u32) -> u32 {
        let aligned = offset & !3;
        let channel_idx = (aligned >> 4) as usize;
        let reg = aligned & 0xF;

        if channel_idx < 7 {
            let ch = &self.channels[channel_idx];
            match reg {
                0x0 => ch.base_addr,
                0x4 => ch.block_ctrl,
                0x8 => ch.ctrl,
                _ => 0,
            }
        } else {
            match aligned {
                0x70 => self.control,
                0x74 => self.read_interrupt(),
                _ => 0,
            }
        }
    }

    pub fn write(&mut self, offset: u32, val: u32, irq: &mut Interrupts) {
        let aligned = offset & !3;
        let channel_idx = (aligned >> 4) as usize;
        let reg = aligned & 0xF;

        if channel_idx < 7 {
            let ch = &mut self.channels[channel_idx];
            match reg {
                0x0 => ch.base_addr = val & 0x00FF_FFFF,
                0x4 => ch.block_ctrl = val,
                0x8 => ch.ctrl = val & 0x7177_0703,
                _ => {}
            }
        } else {
            match aligned {
                0x70 => self.control = val,
                0x74 => self.write_interrupt(val, irq),
                _ => {}
            }
        }
    }

    /// Escrita parcial de 8 bits preservando os outros 3 bytes do registrador
    pub fn write8(&mut self, offset: u32, val: u8, irq: &mut Interrupts) {
        let aligned = offset & !3;
        let shift = (offset & 3) * 8;

        if aligned == 0x74 {
            // Tratamento especial para escrita de byte no DICR (evita dar ACK acidental nos outros bytes!)
            match offset & 3 {
                0 => self.interrupt = (self.interrupt & !0x0000_003F) | ((val as u32) & 0x3F),
                1 => {
                    let prev_irq = self.is_irq_active();
                    self.interrupt = (self.interrupt & !0x0000_8000) | (((val as u32) & 0x80) << 8);
                    if !prev_irq && self.is_irq_active() {
                        irq.trigger(InterruptType::Dma);
                    }
                }
                2 => {
                    let prev_irq = self.is_irq_active();
                    self.interrupt = (self.interrupt & !0x00FF_0000) | ((val as u32) << 16);
                    if !prev_irq && self.is_irq_active() {
                        irq.trigger(InterruptType::Dma);
                    }
                }
                3 => {
                    // Byte 3 (bits 24..31): escrever 1 limpa a flag de IRQ do canal!
                    let ack = (val & 0x7F) as u32;
                    self.interrupt &= !(ack << 24);
                }
                _ => unreachable!(),
            }
            return;
        }

        let cur = self.read(aligned);
        let mask = !(0xFFu32 << shift);
        let merged = (cur & mask) | ((val as u32) << shift);
        self.write(aligned, merged, irq);
    }

    /// Escrita parcial de 16 bits preservando a outra metade do registrador
    pub fn write16(&mut self, offset: u32, val: u16, irq: &mut Interrupts) {
        let aligned = offset & !3;
        if aligned == 0x74 {
            self.write8(offset, val as u8, irq);
            self.write8(offset + 1, (val >> 8) as u8, irq);
            return;
        }

        let shift = (offset & 2) * 8;
        let cur = self.read(aligned);
        let mask = !(0xFFFFu32 << shift);
        let merged = (cur & mask) | ((val as u32) << shift);
        self.write(aligned, merged, irq);
    }

    fn read_interrupt(&self) -> u32 {
        let mut r = self.interrupt & 0x7FFF_803F;
        if self.is_irq_active() {
            r |= 1 << 31;
        }
        r
    }

    fn write_interrupt(&mut self, val: u32, irq: &mut Interrupts) {
        let prev_irq = self.is_irq_active();

        let ack = (val >> 24) & 0x7F;
        self.interrupt &= !(ack << 24);

        let mask = val & 0x00FF_803F;
        self.interrupt = (self.interrupt & !0x00FF_803F) | mask;

        if !prev_irq && self.is_irq_active() {
            irq.trigger(InterruptType::Dma);
        }
    }

    pub fn is_irq_active(&self) -> bool {
        let force = (self.interrupt & (1 << 15)) != 0;
        let master_enable = (self.interrupt & (1 << 23)) != 0;
        let channel_enabled = (self.interrupt >> 16) & 0x7F;
        let channel_flags = (self.interrupt >> 24) & 0x7F;

        force || (master_enable && (channel_enabled & channel_flags) != 0)
    }

    pub fn trigger_done(&mut self, port: DmaPort, irq: &mut Interrupts) {
        let port_idx = port as usize;
        self.channels[port_idx].finish();

        let prev_irq = self.is_irq_active();
        let irq_enabled = (self.interrupt & (1 << (16 + port_idx))) != 0;
        if irq_enabled {
            self.interrupt |= 1 << (24 + port_idx);
            if !prev_irq && self.is_irq_active() {
                irq.trigger(InterruptType::Dma);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dma_registers_and_channel_active() {
        let mut dma = Dma::new();
        let mut irq = Interrupts::new();

        dma.write(0x20, 0x0000_1000, &mut irq);
        dma.write(0x24, 32, &mut irq);
        dma.write(0x28, (1 << 24) | (1 << 28), &mut irq);

        assert_eq!(dma.read(0x20), 0x0000_1000);
        assert_eq!(dma.read(0x24), 32);
        assert!(dma.channels[DmaPort::Gpu as usize].is_active());

        dma.channels[DmaPort::Gpu as usize].finish();
        assert!(!dma.channels[DmaPort::Gpu as usize].is_active());
    }

    #[test]
    fn test_dma_partial_16bit_writes() {
        let mut dma = Dma::new();
        let mut irq = Interrupts::new();

        // Escreve BS = 0x0020 na metade baixa de BCR e BC = 0x0004 na metade alta
        dma.write16(0x24, 0x0020, &mut irq);
        dma.write16(0x26, 0x0004, &mut irq);

        // Ambas as metades devem estar preservadas!
        assert_eq!(dma.read(0x24), 0x0004_0020);
    }

    #[test]
    fn test_dma_interrupt_acknowledgment() {
        let mut dma = Dma::new();
        let mut irq = Interrupts::new();

        dma.write(0x74, (1 << 23) | (1 << (16 + 6)), &mut irq);
        assert!(!dma.is_irq_active());

        dma.trigger_done(DmaPort::Otc, &mut irq);
        assert!(dma.is_irq_active());
        assert_eq!(irq.status, 1 << (InterruptType::Dma as u32));

        // Limpa via write8 no byte 3 do DICR (offset 0x77) sem perder a máscara do byte 2!
        dma.write8(0x77, 1 << 6, &mut irq);
        assert!(!dma.is_irq_active());
        assert_ne!(dma.read(0x74) & (1 << 23), 0); // Master enable preservado!
    }
}
