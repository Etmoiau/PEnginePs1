#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum InterruptType {
    VBlank = 0,
    Gpu = 1,
    CdRom = 2,
    Dma = 3,
    Timer0 = 4,
    Timer1 = 5,
    Timer2 = 6,
    PadMemCard = 7,
    Sio = 8,
    Spu = 9,
    Pio = 10,
}

#[derive(Default, Clone, Copy)]
pub struct Interrupts {
    pub status: u16,
    pub mask: u16,
}

impl Interrupts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retorna verdadeiro se alguma interrupção ativa não estiver mascarada.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.status & self.mask) != 0
    }

    /// Dispara uma interrupção de hardware (seta o bit correspondente no I_STAT).
    pub fn trigger(&mut self, interrupt: InterruptType) {
        self.status |= 1 << (interrupt as u32);
    }

    /// No PS1, escrever 0 reconhece (limpa) a interrupção. Escrever 1 mantém o valor atual.
    #[inline(always)]
    pub fn acknowledge(&mut self, val: u16) {
        self.status &= val;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interrupt_masking_and_ack() {
        let mut irq = Interrupts::new();
        irq.trigger(InterruptType::VBlank);
        irq.trigger(InterruptType::Timer0);
        assert_eq!(irq.status, 0x0011);

        assert!(!irq.is_active());
        irq.mask = 1 << 4;
        assert!(irq.is_active());

        irq.acknowledge(!(1 << 4));
        assert_eq!(irq.status, 0x0001);
        assert!(!irq.is_active());
    }
}
