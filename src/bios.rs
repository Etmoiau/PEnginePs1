// src/bios.rs

pub const BIOS_SIZE: usize = 512 * 1024; // 524.288 bytes

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Japan,
    NorthAmerica,
    Europe,
}

#[derive(Debug, Clone)]
pub struct BiosMetadata {
    pub name: &'static str,
    pub version_major: u8,
    pub version_minor: u8,
    pub region: Region,
    pub sha256: [u8; 32],
    pub animation_jump_hook: Option<usize>,
}

pub struct Bios {
    pub data: Box<[u8; BIOS_SIZE]>,
    pub metadata: Option<&'static BiosMetadata>,
    pub sha256_hex: String,
    pub tty_buffer: String,
}

impl Bios {
    pub fn new(raw_bytes: Vec<u8>) -> Result<Self, String> {
        if raw_bytes.len() != BIOS_SIZE {
            return Err(format!(
                "Tamanho de BIOS inválido! Esperado: 512 KB, recebido: {} bytes",
                raw_bytes.len()
            ));
        }

        let mut data = vec![0u8; BIOS_SIZE].into_boxed_slice();
        data.copy_from_slice(&raw_bytes);
        let data_box: Box<[u8; BIOS_SIZE]> = data.try_into().unwrap();

        let hash = sha256_digest(&*data_box);
        let metadata = lookup_database(&hash);
        let sha256_hex = hash.iter().map(|b| format!("{b:02x}")).collect::<String>();

        Ok(Self {
            data: data_box,
            metadata,
            sha256_hex,
            tty_buffer: String::with_capacity(256),
        })
    }

    #[inline(always)]
    pub fn read8(&self, offset: usize) -> u8 {
        self.data[offset & (BIOS_SIZE - 1)]
    }

    #[inline(always)]
    pub fn read16(&self, offset: usize) -> u16 {
        let off = offset & (BIOS_SIZE - 1);
        let b0 = self.data[off] as u16;
        let b1 = self.data[off + 1] as u16;
        b0 | (b1 << 8)
    }

    #[inline(always)]
    pub fn read32(&self, offset: usize) -> u32 {
        let off = offset & (BIOS_SIZE - 1);
        let bytes: [u8; 4] = self.data[off..off + 4].try_into().unwrap();
        u32::from_le_bytes(bytes)
    }

    /// Pula a animação da logo da Sony e vai direto ao ponto
    pub fn patch_skip_boot_logo(&mut self) -> bool {
        if let Some(meta) = self.metadata {
            if let Some(offset) = meta.animation_jump_hook {
                self.data[offset] = 0;
                self.data[offset + 1] = 0;
                self.data[offset + 2] = 0;
                self.data[offset + 3] = 0;
                return true;
            }
        }
        false
    }

    /// Captura as mensagens de depuração (putchar) do Kernel da BIOS
    pub fn handle_tty(&mut self, pc: u32, regs: &[u32; 32]) {
        if (pc & 0x1FFF_FFFF) == 0x0000_00A0 {
            let func = regs[9]; // Registrador $t1 ($9) armazena a função
            if func == 0x3C {
                let char_byte = (regs[4] & 0xFF) as u8; // Registrador $a0 ($4) armazena o caractere
                if char_byte == b'\n' {
                    println!("[BIOS TTY] {}", self.tty_buffer);
                    self.tty_buffer.clear();
                } else if (32..=126).contains(&char_byte) {
                    self.tty_buffer.push(char_byte as char);
                }
            }
        }
    }
}

// =============================================================================
// Banco de Dados das BIOS Oficiais do PlayStation 1
// =============================================================================

static BIOS_DB: &[BiosMetadata] = &[
    BiosMetadata { name: "SCPH-1000 (v1.0 JP)", version_major: 1, version_minor: 0, region: Region::Japan, sha256: [0xcf, 0xc1, 0xfc, 0x38, 0xeb, 0x44, 0x2f, 0x6f, 0x80, 0x78, 0x14, 0x52, 0x11, 0x9e, 0x93, 0x1b, 0xca, 0xe2, 0x81, 0x00, 0xc1, 0xc9, 0x7e, 0x7e, 0x6c, 0x5f, 0x27, 0x25, 0xbb, 0xb0, 0xf8, 0xbb], animation_jump_hook: None },
    BiosMetadata { name: "SCPH-3000 (v1.1 JP)", version_major: 1, version_minor: 1, region: Region::Japan, sha256: [0x5e, 0xb3, 0xae, 0xe4, 0x95, 0x93, 0x75, 0x58, 0x31, 0x2b, 0x83, 0xb5, 0x43, 0x23, 0xd7, 0x6a, 0x4a, 0x01, 0x51, 0x90, 0xde, 0xcd, 0x40, 0x51, 0x21, 0x4f, 0x1b, 0x6d, 0xf0, 0x6a, 0xc3, 0x4b], animation_jump_hook: None },
    BiosMetadata { name: "SCPH-1001 (v2.0 US)", version_major: 2, version_minor: 0, region: Region::NorthAmerica, sha256: [0x42, 0xe4, 0x12, 0x4b, 0xe7, 0x62, 0x3e, 0x2e, 0x28, 0xb1, 0xdb, 0x0d, 0x8d, 0x42, 0x65, 0x39, 0x64, 0x6f, 0xae, 0xe4, 0x9d, 0x74, 0xb7, 0x11, 0x66, 0xd8, 0xba, 0x5b, 0xd7, 0xc4, 0x72, 0xed], animation_jump_hook: None },
    BiosMetadata { name: "SCPH-1002 (v2.0 EU)", version_major: 2, version_minor: 0, region: Region::Europe, sha256: [0x0a, 0xf2, 0xbe, 0x34, 0x68, 0xd3, 0x0b, 0x60, 0x18, 0xb3, 0xc3, 0xb0, 0xd9, 0x8b, 0x8b, 0x64, 0x34, 0x7e, 0x25, 0x5e, 0x16, 0xd8, 0x74, 0xd5, 0x5f, 0x03, 0x63, 0x64, 0x89, 0x73, 0xdb, 0xf0], animation_jump_hook: None },
    BiosMetadata { name: "SCPH-3500 (v2.1 JP)", version_major: 2, version_minor: 1, region: Region::Japan, sha256: [0x6f, 0x71, 0xca, 0x1e, 0x71, 0x6d, 0xa7, 0x61, 0xdc, 0x53, 0x18, 0x7b, 0xd3, 0x9e, 0x00, 0xc2, 0x13, 0xf5, 0x66, 0xe5, 0x50, 0x90, 0x70, 0x8f, 0xd3, 0xe2, 0xb4, 0xb4, 0x25, 0xc8, 0xc9, 0x89], animation_jump_hook: None },
    BiosMetadata { name: "SCPH-1001 (v2.1 US)", version_major: 2, version_minor: 1, region: Region::NorthAmerica, sha256: [0x6a, 0xd5, 0x52, 0x1d, 0x10, 0x5a, 0x6b, 0x86, 0x74, 0x1f, 0x1a, 0xf8, 0xda, 0x2e, 0x6e, 0xa1, 0xc7, 0x32, 0xd3, 0x44, 0x59, 0x94, 0x06, 0x18, 0xc7, 0x03, 0x05, 0xa1, 0x05, 0xe8, 0xec, 0x10], animation_jump_hook: None },
    BiosMetadata { name: "SCPH-1002 (v2.1 EU)", version_major: 2, version_minor: 1, region: Region::Europe, sha256: [0x1e, 0xfb, 0x0c, 0xfc, 0x5d, 0xb8, 0xa8, 0x75, 0x1a, 0x88, 0x4c, 0x53, 0x12, 0xe9, 0xc6, 0x26, 0x5c, 0xa1, 0xbc, 0x58, 0x0d, 0xc0, 0xc2, 0x66, 0x3e, 0xb2, 0xde, 0xa3, 0xbd, 0xe9, 0xfc, 0xf7], animation_jump_hook: None },
    BiosMetadata { name: "SCPH-5000 (v2.2 JP)", version_major: 2, version_minor: 2, region: Region::Japan, sha256: [0x0c, 0x83, 0x59, 0x87, 0x0c, 0xba, 0xc0, 0xea, 0x09, 0x1f, 0x1c, 0x87, 0xf1, 0x88, 0xcd, 0x33, 0x2d, 0xcc, 0x70, 0x97, 0x53, 0xb9, 0x1c, 0xaf, 0xd9, 0xfd, 0x44, 0xa4, 0xa6, 0x18, 0x81, 0x97], animation_jump_hook: None },
    BiosMetadata { name: "DTLH-3000 (v2.2 US)", version_major: 2, version_minor: 2, region: Region::NorthAmerica, sha256: [0x71, 0xaf, 0x94, 0xd1, 0xe4, 0x7a, 0x68, 0xc1, 0x1e, 0x8f, 0xdb, 0x9f, 0x83, 0x68, 0x04, 0x06, 0x01, 0x51, 0x4a, 0x42, 0xa5, 0xa3, 0x99, 0xcd, 0xa4, 0x8c, 0x7d, 0x3b, 0xff, 0x1e, 0x99, 0xd3], animation_jump_hook: None },
    BiosMetadata { name: "DTLH-3002 (v2.2 EU)", version_major: 2, version_minor: 2, region: Region::Europe, sha256: [0x3d, 0x06, 0xd2, 0xc4, 0x69, 0x31, 0x3c, 0x2a, 0x21, 0x28, 0xd2, 0x4f, 0xe2, 0xe0, 0xc7, 0x1f, 0xf9, 0x9b, 0xc2, 0x03, 0x2b, 0xe8, 0x9a, 0x82, 0x9a, 0x62, 0x33, 0x71, 0x87, 0xf5, 0x00, 0xb7], animation_jump_hook: None },
    BiosMetadata { name: "SCPH-5500 (v3.0 JP)", version_major: 3, version_minor: 0, region: Region::Japan, sha256: [0x9c, 0x04, 0x21, 0x85, 0x8e, 0x21, 0x78, 0x05, 0xf4, 0xab, 0xe1, 0x86, 0x98, 0xaf, 0xea, 0x8d, 0x5a, 0xa3, 0x6f, 0xf0, 0x72, 0x7e, 0xeb, 0x48, 0x49, 0x44, 0xe0, 0x0e, 0xb5, 0xe7, 0xea, 0xdb], animation_jump_hook: None },
    BiosMetadata { name: "SCPH-1001 / 5501 (v3.0 US)", version_major: 3, version_minor: 0, region: Region::NorthAmerica, sha256: [0x11, 0x05, 0x2b, 0x64, 0x99, 0xe4, 0x66, 0xbb, 0xf0, 0xa7, 0x09, 0xb1, 0xf9, 0xcb, 0x68, 0x34, 0xa9, 0x41, 0x8e, 0x66, 0x68, 0x03, 0x87, 0x91, 0x24, 0x51, 0xe9, 0x71, 0xcf, 0x8a, 0x1f, 0xef], animation_jump_hook: Some(0x6990) },
    BiosMetadata { name: "SCPH-1002 / 5502 (v3.0 EU)", version_major: 3, version_minor: 0, region: Region::Europe, sha256: [0x1f, 0xaa, 0xa1, 0x8f, 0xa8, 0x20, 0xa0, 0x22, 0x5e, 0x48, 0x8d, 0x9f, 0x08, 0x62, 0x96, 0xb8, 0xe6, 0xc4, 0x6d, 0xf7, 0x39, 0x66, 0x60, 0x93, 0x98, 0x7f, 0xf7, 0xd8, 0xfd, 0x35, 0x2c, 0x09], animation_jump_hook: None },
    BiosMetadata { name: "SCPH-7000 (v4.0 JP)", version_major: 4, version_minor: 0, region: Region::Japan, sha256: [0xe9, 0x00, 0x50, 0x4d, 0x17, 0x55, 0xf0, 0x21, 0xf8, 0x61, 0xb8, 0x2c, 0x82, 0x58, 0xc5, 0xe6, 0x65, 0x8c, 0x7b, 0x59, 0x2f, 0x80, 0x0c, 0xcc, 0xd9, 0x1f, 0x5d, 0x32, 0xea, 0x38, 0x0d, 0x28], animation_jump_hook: Some(0x6990) },
    BiosMetadata { name: "SCPH-7001 / 9001 (v4.1 US)", version_major: 4, version_minor: 1, region: Region::NorthAmerica, sha256: [0x39, 0xdc, 0xc1, 0xa0, 0x71, 0x70, 0x36, 0xc9, 0xb6, 0xac, 0x52, 0xfe, 0xfd, 0x1e, 0xe7, 0xa5, 0x7d, 0x38, 0x08, 0xe8, 0xcf, 0xbc, 0x75, 0x58, 0x79, 0xfa, 0x68, 0x5a, 0x0a, 0x73, 0x82, 0x78], animation_jump_hook: Some(0x6990) },
    BiosMetadata { name: "SCPH-7002 / 7502 (v4.1 EU)", version_major: 4, version_minor: 1, region: Region::Europe, sha256: [0x5e, 0x84, 0xa9, 0x48, 0x18, 0xcf, 0x52, 0x82, 0xf4, 0x21, 0x75, 0x91, 0xfe, 0xfd, 0x88, 0xbe, 0x36, 0xb9, 0xb1, 0x74, 0xb3, 0xcc, 0x7c, 0xb0, 0xbc, 0xd7, 0x51, 0x99, 0xbe, 0xb4, 0x50, 0xf1], animation_jump_hook: Some(0x6990) },
    BiosMetadata { name: "SCPH-9000 (v4.3 JP)", version_major: 4, version_minor: 3, region: Region::Japan, sha256: [0xb2, 0x9b, 0x4b, 0x5f, 0xcd, 0xde, 0xf3, 0x69, 0xbd, 0x66, 0x40, 0xac, 0xac, 0xda, 0x08, 0x65, 0xe0, 0x36, 0x6f, 0xcf, 0x7e, 0xa5, 0x4e, 0x40, 0xb2, 0xf1, 0xa8, 0x17, 0x80, 0x04, 0xf8, 0x9a], animation_jump_hook: None },
    BiosMetadata { name: "SCPH-9002 (v4.4 EU)", version_major: 4, version_minor: 4, region: Region::Europe, sha256: [0x5c, 0x01, 0x66, 0xda, 0x24, 0xe2, 0x7d, 0xea, 0xa8, 0x22, 0x46, 0xde, 0x8f, 0xf0, 0x10, 0x82, 0x67, 0xfe, 0x4b, 0xb5, 0x9f, 0x6d, 0xf0, 0xfd, 0xec, 0x50, 0xe0, 0x5e, 0x62, 0x44, 0x8c, 0xa4], animation_jump_hook: None },
    BiosMetadata { name: "PSone SCPH-101 (v4.5 US)", version_major: 4, version_minor: 5, region: Region::NorthAmerica, sha256: [0xac, 0xa9, 0xcb, 0xfa, 0x97, 0x4b, 0x93, 0x36, 0x46, 0xba, 0xad, 0x65, 0x56, 0xa8, 0x67, 0xec, 0xa9, 0xb8, 0x1c, 0xe6, 0x5d, 0x8a, 0xf3, 0x43, 0xa7, 0x84, 0x3f, 0x77, 0x75, 0xb9, 0xff, 0xc8], animation_jump_hook: None },
    BiosMetadata { name: "PSone SCPH-102 (v4.5 EU)", version_major: 4, version_minor: 4, region: Region::Europe, sha256: [0x42, 0x24, 0x4b, 0x0c, 0x65, 0x08, 0x21, 0x51, 0x97, 0x51, 0xb7, 0xe7, 0x7a, 0xd1, 0xd3, 0x22, 0x2a, 0x01, 0x25, 0xe7, 0x55, 0x86, 0xdf, 0x2b, 0x4e, 0x84, 0xba, 0x69, 0x3b, 0x98, 0x09, 0xdc], animation_jump_hook: None },
];

fn lookup_database(hash: &[u8; 32]) -> Option<&'static BiosMetadata> {
    BIOS_DB.iter().find(|meta| meta.sha256 == *hash)
}

// =============================================================================
// Motor SHA-256 Nativo
// =============================================================================

fn sha256_digest(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() + 8) % 64 != 0 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, part) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes(part.try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut h_val = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h_val
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h_val = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(h_val);
    }

    let mut result = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        result[i * 4..i * 4 + 4].copy_from_slice(&val.to_be_bytes());
    }
    result
}

// =============================================================================
// Testes Unitários da BIOS
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bios_creation_and_size() {
        let dummy_data = vec![0u8; BIOS_SIZE];
        let bios = Bios::new(dummy_data);
        assert!(bios.is_ok());

        // Tamanho incorreto deve falhar graciosamente
        let invalid_data = vec![0u8; 100];
        let bios_err = Bios::new(invalid_data);
        assert!(bios_err.is_err());
    }

    #[test]
    fn test_bios_read_endianness() {
        let mut data = vec![0u8; BIOS_SIZE];
        data[0] = 0x78;
        data[1] = 0x56;
        data[2] = 0x34;
        data[3] = 0x12;

        let bios = Bios::new(data).unwrap();
        assert_eq!(bios.read8(0), 0x78);
        assert_eq!(bios.read16(0), 0x5678);
        assert_eq!(bios.read32(0), 0x1234_5678);
    }
}

