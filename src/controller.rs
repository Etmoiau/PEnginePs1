// src/controller.rs

use crate::interrupt::{InterruptType, Interrupts};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Select = 0,
    Start = 3,
    Up = 4,
    Right = 5,
    Down = 6,
    Left = 7,
    L2 = 8,
    R2 = 9,
    L1 = 10,
    R1 = 11,
    Triangle = 12,
    Circle = 13,
    Cross = 14,
    Square = 15,
}

#[derive(Clone, Copy, Debug)]
enum BusState {
    Idle,
    Transfer(u8, bool, u32), // (byte_resposta, dsr_ack, ciclos_restantes)
    Dsr(u32),                // Pulso DSR/ACK ativo pelos ciclos restantes
}

pub struct Controller {
    pub mode: u16,
    pub ctrl: u16,
    pub baud: u16,
    pub rx_data: u8,
    pub rx_ready: bool,

    seq: u8,
    active: bool,
    pub target_memcard: bool,
    pub memcard: crate::memcard::MemoryCard,
    pub rumble_small: u8,
    pub rumble_large: u8,
    pub ack: bool,
    pub irq_flag: bool,
    bus_state: BusState,

    // Eixos analógicos (0x00..=0xFF, centro = 0x80)
    pub is_analog: bool,
    pub left_x: u8,
    pub left_y: u8,
    pub right_x: u8,
    pub right_y: u8,

    pub buttons: u16,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            mode: 0x000D,
            ctrl: 0,
            baud: 0x0088,
            rx_data: 0xFF,
            rx_ready: false,
            seq: 0,
            active: false,
            target_memcard: false,
            memcard: crate::memcard::MemoryCard::new("memcard1.mcr"),
            rumble_small: 0,
            rumble_large: 0,
            ack: false,
            irq_flag: false,
            bus_state: BusState::Idle,
            is_analog: false,
            left_x: 0x80,
            left_y: 0x80,
            right_x: 0x80,
            right_y: 0x80,
            buttons: 0xFFFF,
        }
    }

    pub fn press_button(&mut self, button: Button) {
        self.buttons &= !(1 << (button as usize));
    }

    pub fn release_button(&mut self, button: Button) {
        self.buttons |= 1 << (button as usize);
    }

    pub fn tick(&mut self, cycles: u32, irq: &mut Interrupts) {
        match self.bus_state {
            BusState::Idle => {}
            BusState::Transfer(resp, dsr, rem) => {
                if rem <= cycles {
                    self.rx_data = resp;
                    self.rx_ready = true;
                    self.ack = dsr;

                    if dsr {
                        if (self.ctrl & (1 << 12)) != 0 {
                            if !self.irq_flag {
                                irq.trigger(InterruptType::PadMemCard);
                            }
                            self.irq_flag = true;
                        }
                        // Pulso DSR curto de 10 ciclos (idêntico ao Rustation)
                        self.bus_state = BusState::Dsr(10);
                    } else {
                        self.bus_state = BusState::Idle;
                    }
                } else {
                    self.bus_state = BusState::Transfer(resp, dsr, rem - cycles);
                }
            }
            BusState::Dsr(rem) => {
                if rem <= cycles {
                    self.ack = false;
                    self.bus_state = BusState::Idle;
                } else {
                    self.bus_state = BusState::Dsr(rem - cycles);
                }
            }
        }
    }

    pub fn read_status(&self) -> u32 {
        let mut stat = 0x0005u32; // Bit 0 (TX Ready 1) + Bit 2 (TX Ready 2)
        if self.rx_ready {
            stat |= 1 << 1; // Bit 1: RX FIFO Not Empty
        }
        if self.ack {
            stat |= 1 << 7; // Bit 7: /ACK (DSR)
        }
        if self.irq_flag {
            stat |= 1 << 9; // Bit 9: Interrupt Request
        }
        stat
    }

    pub fn write_ctrl(&mut self, val: u16, irq: &mut Interrupts) {
        if (val & 0x0040) != 0 {
            // Soft Reset
            self.mode = 0;
            self.ctrl = 0;
            self.baud = 0x0088;
            self.seq = 0;
            self.active = false;
            self.target_memcard = false;
            self.memcard.reset_transfer();
            self.rx_ready = false;
            self.ack = false;
            self.irq_flag = false;
            self.bus_state = BusState::Idle;
            return;
        }

        if (val & 0x0010) != 0 {
            // Interrupt Acknowledge
            self.irq_flag = false;
            if self.ack && (self.ctrl & (1 << 12)) != 0 {
                self.irq_flag = true;
                irq.trigger(InterruptType::PadMemCard);
            }
        }

        let prev_select = (self.ctrl & 0x0002) != 0;
        let prev_slot = (self.ctrl >> 13) & 1;

        // Preserva bits graváveis (TXEN, SELECT, RXEN, Unknown 3 e 5, Interrupts, Slot)
        self.ctrl = val & 0x3F2F;

        let new_select = (self.ctrl & 0x0002) != 0;
        let new_slot = (self.ctrl >> 13) & 1;

        if !new_select {
            self.seq = 0;
            self.active = false;
            self.target_memcard = false;
            self.memcard.reset_transfer();
            self.ack = false;
            self.bus_state = BusState::Idle;
        } else if !prev_select || prev_slot != new_slot {
            // Borda de subida do SELECT: habilita o dispositivo para uma nova transação
            self.seq = 0;
            self.active = true;
            self.target_memcard = false;
            self.memcard.reset_transfer();
        }
    }

    pub fn write_data(&mut self, byte: u8, _irq: &mut Interrupts) {
        if (self.ctrl & 0x0001) == 0 {
            return;
        }

        self.ack = false;

        let selected = (self.ctrl & 0x0002) != 0;
        let is_slot_2 = (self.ctrl & 0x2000) != 0;

        let (resp, dsr) = if !selected || is_slot_2 || !self.active {
            (0xFF, false)
        } else if self.seq == 0 {
            self.seq = 1;
            if byte == 0x01 {
                // Selecionou Controle (Port 1)
                self.target_memcard = false;
                self.active = true;
                (0xFF, true)
            } else if byte == 0x81 {
                // Selecionou Memory Card (Port 1)
                self.target_memcard = true;
                self.active = true;
                self.memcard.reset_transfer();
                (0xFF, true)
            } else {
                self.active = false;
                (0xFF, false)
            }
        } else if self.target_memcard {
            let (r, ack_pulse) = self.memcard.process_byte(byte);
            self.active = ack_pulse;
            (r, ack_pulse)
        } else {
            // Transmissão com o Gamepad (DualShock / Digital)
            let (r, ack_pulse) = match self.seq {
                1 => (if self.is_analog { 0x73 } else { 0x41 }, byte == 0x42),
                2 => (0x5A, true),
                3 => {
                    if self.is_analog {
                        self.rumble_small = byte;
                    }
                    (self.buttons as u8, true)
                }
                4 => {
                    if self.is_analog {
                        self.rumble_large = byte;
                    }
                    ((self.buttons >> 8) as u8, self.is_analog)
                }
                5 if self.is_analog => (self.right_x, true),
                6 if self.is_analog => (self.right_y, true),
                7 if self.is_analog => (self.left_x, true),
                8 if self.is_analog => (self.left_y, false),
                _ => (0xFF, false),
            };
            self.seq += 1;
            self.active = ack_pulse;
            (r, ack_pulse)
        };

        // Duração da transferência serial de 8 bits: 8 * baud_div
        let tx_cycles = 8 * (self.baud as u32).max(0x40);
        self.bus_state = BusState::Transfer(resp, dsr, tx_cycles);
    }

    /// Retorna (motor_esquerdo_forte, motor_direito_fraco) para force feedback no XInput
    pub fn get_rumble(&self) -> (u16, u16) {
        if !self.is_analog {
            return (0, 0);
        }
        let left = (self.rumble_large as u16) << 8;
        let right = if self.rumble_small != 0 { 0xFFFF } else { 0 };
        (left, right)
    }

    pub fn read_data(&mut self) -> u8 {
        self.rx_ready = false;
        let v = self.rx_data;
        self.rx_data = 0xFF;
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_handshake_and_buttons() {
        let mut pad = Controller::new();
        let mut irq = Interrupts::new();

        pad.write_ctrl((1 << 0) | (1 << 1) | (1 << 12), &mut irq);

        pad.write_data(0x01, &mut irq);
        pad.tick(1500, &mut irq);
        assert_eq!(pad.read_data(), 0xFF);
        assert!(pad.ack);
        assert!(irq.status & (1 << (InterruptType::PadMemCard as u32)) != 0);

        pad.write_data(0x42, &mut irq);
        pad.tick(1500, &mut irq);
        assert_eq!(pad.read_data(), 0x41);
        assert!(pad.ack);

        pad.write_data(0x00, &mut irq);
        pad.tick(1500, &mut irq);
        assert_eq!(pad.read_data(), 0x5A);
        assert!(pad.ack);

        pad.write_data(0x00, &mut irq);
        pad.tick(1500, &mut irq);
        assert_eq!(pad.read_data(), 0xFF);
        assert!(pad.ack);

        pad.write_data(0x00, &mut irq);
        pad.tick(1500, &mut irq);
        assert_eq!(pad.read_data(), 0xFF);
        assert!(!pad.ack);
    }

    #[test]
    fn test_button_press() {
        let mut pad = Controller::new();
        let mut irq = Interrupts::new();

        pad.press_button(Button::Cross);
        pad.press_button(Button::Start);

        pad.write_ctrl((1 << 0) | (1 << 1), &mut irq);

        pad.write_data(0x01, &mut irq);
        pad.tick(1500, &mut irq);
        pad.read_data();

        pad.write_data(0x42, &mut irq);
        pad.tick(1500, &mut irq);
        pad.read_data();

        pad.write_data(0x00, &mut irq);
        pad.tick(1500, &mut irq);
        pad.read_data();

        pad.write_data(0x00, &mut irq);
        pad.tick(1500, &mut irq);
        let b0 = pad.read_data();
        assert_eq!(b0 & (1 << 3), 0);

        pad.write_data(0x00, &mut irq);
        pad.tick(1500, &mut irq);
        let b1 = pad.read_data();
        assert_eq!(b1 & (1 << 6), 0);
    }
    #[test]
    fn test_memory_card_status_and_read() {
        let mut pad = Controller::new();
        let mut irq = Interrupts::new();

        pad.write_ctrl((1 << 0) | (1 << 1), &mut irq);

        // Envia 0x81 para selecionar o Memory Card
        pad.write_data(0x81, &mut irq);
        pad.tick(1500, &mut irq);
        assert_eq!(pad.read_data(), 0xFF);
        assert!(pad.ack);

        // Envia 'S' (0x53) para consultar Status
        pad.write_data(0x53, &mut irq);
        pad.tick(1500, &mut irq);
        assert_eq!(pad.read_data(), 0x5A);
        assert!(pad.ack);

        // Próximo byte deve retornar 0x5D
        pad.write_data(0x00, &mut irq);
        pad.tick(1500, &mut irq);
        assert_eq!(pad.read_data(), 0x5D);
        assert!(pad.ack);

        // Próximo byte deve retornar flags (0x00) e encerrar transferência (ack false)
        pad.write_data(0x00, &mut irq);
        pad.tick(1500, &mut irq);
        assert_eq!(pad.read_data(), 0x00);
        assert!(!pad.ack);
    }

    #[test]
    fn test_dualshock_rumble_feedback() {
        let mut pad = Controller::new();
        let mut irq = Interrupts::new();
        pad.is_analog = true;

        pad.write_ctrl((1 << 0) | (1 << 1), &mut irq);

        pad.write_data(0x01, &mut irq);
        pad.tick(1500, &mut irq);

        pad.write_data(0x42, &mut irq);
        pad.tick(1500, &mut irq);

        pad.write_data(0x00, &mut irq);
        pad.tick(1500, &mut irq);

        // Byte 3: Small Motor (1 = ligar)
        pad.write_data(0x01, &mut irq);
        pad.tick(1500, &mut irq);

        // Byte 4: Large Motor (128 = meia força)
        pad.write_data(128, &mut irq);
        pad.tick(1500, &mut irq);

        let (left, right) = pad.get_rumble();
        assert_eq!(left, 128 << 8);
        assert_eq!(right, 0xFFFF);
    }
}
