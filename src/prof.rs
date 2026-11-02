// src/prof.rs
// Perfilador leve baseado em RDTSC (usado apenas no modo --bench).
// Custo ~20 ciclos por medição, desligado por padrão.

pub static mut ENABLED: bool = false;
pub static mut COUNTERS: [u64; 8] = [0; 8];

pub const CPU: usize = 0;
pub const TIMERS: usize = 1;
pub const CDROM: usize = 2;
pub const PAD: usize = 3;
pub const SPU: usize = 4;
pub const DMA: usize = 5;
pub const MDEC: usize = 6;

pub const NAMES: [&str; 8] = ["cpu", "timers", "cdrom", "pad", "spu", "dma", "mdec", "-"];

#[inline(always)]
pub fn now() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

#[inline(always)]
pub fn enabled() -> bool {
    unsafe { ENABLED }
}

#[inline(always)]
pub fn add(slot: usize, ticks: u64) {
    unsafe {
        COUNTERS[slot] = COUNTERS[slot].wrapping_add(ticks);
    }
}
