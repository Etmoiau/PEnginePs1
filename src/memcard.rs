// src/memcard.rs
// Emulação do Memory Card oficial de 128 KB (131.072 bytes) do PlayStation 1
// Suporta comandos padrão de leitura ('R'), escrita ('W') e status ('S')
// com sincronização automática para o arquivo físico 'memcard1.mcr'.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

pub const MEMCARD_SIZE: usize = 128 * 1024; // 131.072 bytes (1024 frames de 128 bytes)
pub const FRAME_SIZE: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum McCommand {
    None,
    Read,
    Write,
    Status,
}

pub struct MemoryCard {
    pub data: Box<[u8; MEMCARD_SIZE]>,
    pub file_path: String,
    pub dirty: bool,

    command: McCommand,
    step: usize,
    frame_addr: usize,
    data_idx: usize,
    checksum: u8,
}

impl MemoryCard {
    pub fn new(file_path: &str) -> Self {
        let mut data: Box<[u8; MEMCARD_SIZE]> = vec![0u8; MEMCARD_SIZE]
            .into_boxed_slice()
            .try_into()
            .unwrap();

        let path = Path::new(file_path);
        if path.exists() {
            if let Ok(mut f) = fs::File::open(path) {
                let _ = f.read_exact(&mut data[..]);
                println!("Memory Card carregado: {} (128 KB)", file_path);
            }
        } else {
            // Inicializa e formata um Memory Card novo padrão Sony
            Self::format_card(&mut data);
            if let Ok(mut f) = fs::File::create(path) {
                let _ = f.write_all(&data[..]);
                println!("Novo Memory Card formatado criado: {}", file_path);
            }
        }

        Self {
            data,
            file_path: file_path.to_string(),
            dirty: false,
            command: McCommand::None,
            step: 0,
            frame_addr: 0,
            data_idx: 0,
            checksum: 0,
        }
    }

    /// Formata os blocos e diretório do Memory Card no padrão nativo do BIOS da Sony
    fn format_card(data: &mut [u8; MEMCARD_SIZE]) {
        // Frame 0: Header
        data[0] = b'M';
        data[1] = b'C';
        let mut xor_chk = 0u8;
        for i in 0..127 {
            xor_chk ^= data[i];
        }
        data[127] = xor_chk;

        // Frames 1..=15: Directory entries vazias
        for frame in 1..=15 {
            let base = frame * FRAME_SIZE;
            data[base] = 0xA0; // Bloco formatado e disponível
            let mut chk = 0u8;
            for i in 0..127 {
                chk ^= data[base + i];
            }
            data[base + 127] = chk;
        }
    }

    /// Salva dados pendentes para o arquivo físico no disco
    pub fn flush(&mut self) {
        if self.dirty {
            if let Ok(mut f) = OpenOptions::new().write(true).create(true).open(&self.file_path) {
                let _ = f.write_all(&self.data[..]);
                self.dirty = false;
            }
        }
    }

    pub fn reset_transfer(&mut self) {
        self.command = McCommand::None;
        self.step = 0;
        self.frame_addr = 0;
        self.data_idx = 0;
        self.checksum = 0;
    }

    /// Processa 1 byte da transmissão serial SIO do Memory Card (0x81)
    /// Retorna `(byte_resposta, flag_ack)`
    pub fn process_byte(&mut self, byte: u8) -> (u8, bool) {
        match self.command {
            McCommand::None => {
                match byte {
                    0x52 => {
                        // Comando 'R': Ler frame
                        self.command = McCommand::Read;
                        self.step = 1;
                        (0x5A, true)
                    }
                    0x57 => {
                        // Comando 'W': Gravar frame
                        self.command = McCommand::Write;
                        self.step = 1;
                        (0x5A, true)
                    }
                    0x53 => {
                        // Comando 'S': Status do Memory Card
                        self.command = McCommand::Status;
                        self.step = 1;
                        (0x5A, true)
                    }
                    _ => {
                        self.reset_transfer();
                        (0xFF, false)
                    }
                }
            }

            McCommand::Status => {
                match self.step {
                    1 => {
                        self.step = 2;
                        (0x5D, true)
                    }
                    2 => {
                        self.reset_transfer();
                        (0x00, false)
                    }
                    _ => {
                        self.reset_transfer();
                        (0xFF, false)
                    }
                }
            }

            McCommand::Read => {
                match self.step {
                    1 => {
                        self.step = 2;
                        (0x5D, true)
                    }
                    2 => {
                        self.frame_addr = ((byte as usize) & 0x03) << 8;
                        self.checksum = byte;
                        self.step = 3;
                        (0x00, true)
                    }
                    3 => {
                        self.frame_addr |= byte as usize;
                        self.checksum ^= byte;
                        self.step = 4;
                        (0x00, true)
                    }
                    4 => {
                        self.step = 5;
                        (0x5C, true)
                    }
                    5 => {
                        self.step = 6;
                        (0x5D, true)
                    }
                    6 => {
                        self.step = 7;
                        ((self.frame_addr >> 8) as u8, true)
                    }
                    7 => {
                        self.step = 8;
                        self.data_idx = 0;
                        ((self.frame_addr & 0xFF) as u8, true)
                    }
                    8 => {
                        let offset = (self.frame_addr & 0x3FF) * FRAME_SIZE + self.data_idx;
                        let val = self.data[offset];
                        self.checksum ^= val;
                        self.data_idx += 1;

                        if self.data_idx >= FRAME_SIZE {
                            self.step = 9;
                        }
                        (val, true)
                    }
                    9 => {
                        self.step = 10;
                        (self.checksum, true)
                    }
                    10 => {
                        self.reset_transfer();
                        (b'G', false)
                    }
                    _ => {
                        self.reset_transfer();
                        (0xFF, false)
                    }
                }
            }

            McCommand::Write => {
                match self.step {
                    1 => {
                        self.step = 2;
                        (0x5D, true)
                    }
                    2 => {
                        self.frame_addr = ((byte as usize) & 0x03) << 8;
                        self.checksum = byte;
                        self.step = 3;
                        (0x00, true)
                    }
                    3 => {
                        self.frame_addr |= byte as usize;
                        self.checksum ^= byte;
                        self.step = 4;
                        self.data_idx = 0;
                        (0x00, true)
                    }
                    4 => {
                        let offset = (self.frame_addr & 0x3FF) * FRAME_SIZE + self.data_idx;
                        self.data[offset] = byte;
                        self.checksum ^= byte;
                        self.data_idx += 1;

                        if self.data_idx >= FRAME_SIZE {
                            self.step = 5;
                        }
                        (0x00, true)
                    }
                    5 => {
                        self.step = 6;
                        self.dirty = true;
                        self.flush();
                        (0x00, true)
                    }
                    6 => {
                        self.step = 7;
                        (0x5C, true)
                    }
                    7 => {
                        self.step = 8;
                        (0x5D, true)
                    }
                    8 => {
                        self.reset_transfer();
                        (b'G', false)
                    }
                    _ => {
                        self.reset_transfer();
                        (0xFF, false)
                    }
                }
            }
        }
    }
}
