// src/timers.rs

use crate::interrupt::{InterruptType, Interrupts};

#[derive(Default, Clone, Copy)]
pub struct Timer {
    pub counter: u16,
    pub mode: u16,
    pub target: u16,
    pub sub_cycles: u32,
    target_reached: bool,
    overflow_reached: bool,
    interrupt_fired: bool,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            mode: 0x0400,
            ..Default::default()
        }
    }
}

pub struct Timers {
    pub timers: [Timer; 3],
}

impl Timers {
    pub fn new() -> Self {
        Self {
            timers: [Timer::new(), Timer::new(), Timer::new()],
        }
    }

    pub fn tick(&mut self, cycles: u32, irq: &mut Interrupts) {
        for (i, timer) in self.timers.iter_mut().enumerate() {
            let clock_source = (timer.mode >> 8) & 3;

            let step = if i == 1 && (clock_source == 1 || clock_source == 3) {
                // Timer 1 em modo HSync (~2148 ciclos de CPU por linha de vídeo)
                timer.sub_cycles += cycles;
                let lines = timer.sub_cycles / 2146;
                timer.sub_cycles %= 2146;
                lines as u16
            } else if i == 2 && (clock_source == 2 || clock_source == 3) {
                // Timer 2 em modo SysClock / 8
                timer.sub_cycles += cycles;
                let ticks = timer.sub_cycles / 8;
                timer.sub_cycles %= 8;
                ticks as u16
            } else {
                cycles as u16
            };

            if step == 0 {
                continue;
            }

            let prev_counter = timer.counter;
            let (new_counter, overflowed) = prev_counter.overflowing_add(step);
            timer.counter = new_counter;

            let hit_target = if timer.target > 0 {
                prev_counter < timer.target && timer.counter >= timer.target
            } else {
                overflowed
            };

            if hit_target {
                timer.target_reached = true;

                if (timer.mode & (1 << 3)) != 0 && timer.target > 0 {
                    timer.counter %= timer.target;
                }

                if (timer.mode & (1 << 4)) != 0 {
                    Self::trigger_timer_irq(i, timer, irq);
                }
            }

            if overflowed {
                timer.overflow_reached = true;

                if (timer.mode & (1 << 5)) != 0 {
                    Self::trigger_timer_irq(i, timer, irq);
                }
            }
        }
    }

    fn trigger_timer_irq(index: usize, timer: &mut Timer, irq: &mut Interrupts) {
        if (timer.mode & (1 << 6)) == 0 && timer.interrupt_fired {
            return;
        }

        let irq_type = match index {
            0 => InterruptType::Timer0,
            1 => InterruptType::Timer1,
            _ => InterruptType::Timer2,
        };

        irq.trigger(irq_type);
        timer.interrupt_fired = true;
        timer.mode &= !(1 << 10);
    }

    pub fn read(&mut self, offset: u32) -> u32 {
        let timer_idx = ((offset >> 4) & 3) as usize;
        let reg = offset & 0xF;
        if timer_idx > 2 {
            return 0;
        }

        let timer = &mut self.timers[timer_idx];
        match reg {
            0x0 => timer.counter as u32,
            0x4 => {
                let mut mode = timer.mode;
                if timer.target_reached {
                    mode |= 1 << 11;
                }
                if timer.overflow_reached {
                    mode |= 1 << 12;
                }
                timer.target_reached = false;
                timer.overflow_reached = false;
                timer.mode |= 1 << 10;
                mode as u32
            }
            0x8 => timer.target as u32,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, val: u32) {
        let timer_idx = ((offset >> 4) & 3) as usize;
        let reg = offset & 0xF;
        if timer_idx > 2 {
            return;
        }

        let timer = &mut self.timers[timer_idx];
        match reg {
            0x0 => timer.counter = val as u16,
            0x4 => {
                timer.mode = (val as u16) | 0x0400;
                timer.counter = 0;
                timer.sub_cycles = 0;
                timer.interrupt_fired = false;
            }
            0x8 => timer.target = val as u16,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timer_target_reset_and_irq() {
        let mut timers = Timers::new();
        let mut irq = Interrupts::new();

        timers.write(0x08, 100);
        timers.write(0x04, (1 << 3) | (1 << 4));

        timers.tick(50, &mut irq);
        assert_eq!(timers.read(0x00), 50);
        assert!(!irq.is_active());

        timers.tick(50, &mut irq);
        assert_eq!(timers.read(0x00), 0);
        assert_eq!(irq.status, 1 << 4);
    }

    #[test]
    fn test_timer_overflow_wrap() {
        let mut timers = Timers::new();
        let mut irq = Interrupts::new();

        timers.write(0x10, 0xFFF0);
        timers.tick(32, &mut irq);
        assert_eq!(timers.read(0x10), 0x0010);
    }
}
