// src/mdec.rs

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputDepth {
    Bpp4 = 0,
    Bpp8 = 1,
    Bpp24 = 2,
    Bpp15 = 3,
}

/// Tabela oficial de varredura Zig-Zag (8x8) do hardware MDEC do PS1
const ZIGZAG: [usize; 64] = [
     0,  1,  8, 16,  9,  2,  3, 10,
    17, 24, 32, 25, 18, 11,  4,  5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13,  6,  7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

/// Matriz de escala IDCT 1D oficial da Sony (cossenos em ponto fixo s0.15)
const DEFAULT_IDCT_TABLE: [i16; 64] = [
     23170,  23170,  23170,  23170,  23170,  23170,  23170,  23170,
     32138,  27245,  18204,   6392,  -6393, -18205, -27246, -32139,
     30273,  12539, -12540, -30274, -30274, -12540,  12539,  30273,
     27245,  -6393, -32139, -18205,  18204,  32138,   6392, -27246,
     23170, -23171, -23171,  23170,  23170, -23171, -23171,  23170,
     18204, -32139,   6392,  27245, -27246,  -6393,  32138, -18205,
     12539, -30274,  30273, -12540, -12540,  30273, -30274,  12539,
      6392, -18205,  27245, -32139,  32138, -27246,  18204,  -6393,
];

pub struct Mdec {
    pub luma_quant: [u8; 64],
    pub chroma_quant: [u8; 64],
    pub idct_table: [i16; 64],

    pub output_depth: OutputDepth,
    pub output_signed: bool,
    pub output_bit15: bool,
    pub dma_in_enable: bool,
    pub dma_out_enable: bool,
    pub current_block: u8,

    current_command: u32,
    words_remaining: u32,
    param_index: usize,

    in_fifo: Vec<u16>,
    pub out_fifo: Vec<u32>,
    out_read_idx: usize,
}

impl Mdec {
    pub fn new() -> Self {
        Self {
            luma_quant: [8; 64],
            chroma_quant: [8; 64],
            idct_table: DEFAULT_IDCT_TABLE,
            output_depth: OutputDepth::Bpp15,
            output_signed: false,
            output_bit15: false,
            dma_in_enable: false,
            dma_out_enable: false,
            current_block: 4,
            current_command: 0,
            words_remaining: 0,
            param_index: 0,
            in_fifo: Vec::with_capacity(2048),
            out_fifo: Vec::with_capacity(1024),
            out_read_idx: 0,
        }
    }

    /// Registrador de Status MDEC1 (0x1F80_1824)
    pub fn read_status(&self) -> u32 {
        let mut stat = 0u32;

        if self.words_remaining > 0 {
            stat |= (self.words_remaining - 1) & 0xFFFF;
        } else {
            stat |= 0xFFFF;
        }

        stat |= ((self.current_block & 7) as u32) << 16;
        stat |= (self.output_bit15 as u32) << 23;
        stat |= (self.output_signed as u32) << 24;
        stat |= (self.output_depth as u32) << 25;

        let has_output = self.out_read_idx < self.out_fifo.len();
        if self.dma_out_enable && has_output {
            stat |= 1 << 27; // Data Out Request (DMA1)
        }
        if self.dma_in_enable && self.words_remaining > 0 {
            stat |= 1 << 28; // Data In Request (DMA0)
        }
        if self.words_remaining > 0 || has_output {
            stat |= 1 << 29; // Command Busy
        }
        if !has_output {
            stat |= 1 << 31; // Data Out FIFO Empty
        }

        stat
    }

    /// Leitura de palavra de 32 bits em MDEC0 (0x1F80_1820)
    pub fn read_data(&mut self) -> u32 {
        if self.out_read_idx < self.out_fifo.len() {
            let val = self.out_fifo[self.out_read_idx];
            self.out_read_idx += 1;
            if self.out_read_idx >= self.out_fifo.len() {
                self.out_fifo.clear();
                self.out_read_idx = 0;
            }
            val
        } else {
            0
        }
    }

    /// Escrita de controle/reset em MDEC1 (0x1F80_1824)
    pub fn write_control(&mut self, val: u32) {
        if (val & (1 << 31)) != 0 {
            self.current_command = 0;
            self.words_remaining = 0;
            self.param_index = 0;
            self.current_block = 4;
            self.output_depth = OutputDepth::Bpp4;
            self.output_signed = false;
            self.output_bit15 = false;
            self.in_fifo.clear();
            self.out_fifo.clear();
            self.out_read_idx = 0;
        }

        self.dma_in_enable = (val & (1 << 30)) != 0;
        self.dma_out_enable = (val & (1 << 29)) != 0;
    }

    /// Escrita de comando ou dados em MDEC0 (0x1F80_1820)
    pub fn write_command(&mut self, val: u32) {
        if self.words_remaining == 0 {
            let opcode = val >> 29;
            
            self.current_command = opcode;
            self.param_index = 0;

            self.output_depth = match (val >> 27) & 3 {
                0 => OutputDepth::Bpp4,
                1 => OutputDepth::Bpp8,
                2 => OutputDepth::Bpp24,
                _ => OutputDepth::Bpp15,
            };
            self.output_signed = (val & (1 << 26)) != 0;
            self.output_bit15 = (val & (1 << 25)) != 0;

            match opcode {
                1 => {
                    let num_words = val & 0xFFFF;
                    self.words_remaining = if num_words == 0 { 0x10000 } else { num_words };
                    self.in_fifo.clear();
                    self.out_fifo.clear();
                    self.out_read_idx = 0;
                }
                2 => {
                    let has_chroma = (val & 1) != 0;
                    self.words_remaining = if has_chroma { 32 } else { 16 };
                }
                3 => {
                    self.words_remaining = 32;
                }
                _ => {
                    self.words_remaining = 0;
                }
            }
        } else {
            self.words_remaining -= 1;

            match self.current_command {
                1 => {
                    self.in_fifo.push(val as u16);
                    self.in_fifo.push((val >> 16) as u16);
                    self.decode_macroblocks();
                }
                2 => {
                    let bytes = val.to_le_bytes();
                    for b in bytes {
                        if self.param_index < 64 {
                            self.luma_quant[self.param_index] = b;
                        } else if self.param_index < 128 {
                            self.chroma_quant[self.param_index - 64] = b;
                        }
                        self.param_index += 1;
                    }
                }
                3 => {
                    let v0 = val as i16;
                    let v1 = (val >> 16) as i16;
                    if self.param_index < 64 {
                        self.idct_table[self.param_index] = v0;
                        self.idct_table[self.param_index + 1] = v1;
                        self.param_index += 2;
                    }
                }
                _ => {}
            }
        }
    }

    // =========================================================================
    // Pipeline Completo: RLE -> ZigZag -> 2D IDCT -> YCrCb para RGB
    // =========================================================================

    fn decode_macroblocks(&mut self) {
        let mut cursor = 0;

        while cursor < self.in_fifo.len() {
            let mb_start = cursor;
            match self.output_depth {
                OutputDepth::Bpp4 | OutputDepth::Bpp8 => {
                    self.current_block = 4;
                    let mut y_blk = [0i16; 64];
                    if !Self::rl_decode_block(&self.in_fifo, &mut cursor, &mut y_blk, &self.luma_quant) {
                        cursor = mb_start;
                        break;
                    }
                    self.real_idct(&mut y_blk);
                    self.y_to_mono(&y_blk);
                }
                OutputDepth::Bpp15 | OutputDepth::Bpp24 => {
                    let mut cr_blk = [0i16; 64];
                    let mut cb_blk = [0i16; 64];
                    let mut y1_blk = [0i16; 64];
                    let mut y2_blk = [0i16; 64];
                    let mut y3_blk = [0i16; 64];
                    let mut y4_blk = [0i16; 64];

                    self.current_block = 4; // Cr
                    if !Self::rl_decode_block(&self.in_fifo, &mut cursor, &mut cr_blk, &self.chroma_quant) {
                        cursor = mb_start;
                        break;
                    }

                    self.current_block = 5; // Cb
                    if !Self::rl_decode_block(&self.in_fifo, &mut cursor, &mut cb_blk, &self.chroma_quant) {
                        cursor = mb_start;
                        break;
                    }

                    self.current_block = 0; // Y1
                    if !Self::rl_decode_block(&self.in_fifo, &mut cursor, &mut y1_blk, &self.luma_quant) {
                        cursor = mb_start;
                        break;
                    }

                    self.current_block = 1; // Y2
                    if !Self::rl_decode_block(&self.in_fifo, &mut cursor, &mut y2_blk, &self.luma_quant) {
                        cursor = mb_start;
                        break;
                    }

                    self.current_block = 2; // Y3
                    if !Self::rl_decode_block(&self.in_fifo, &mut cursor, &mut y3_blk, &self.luma_quant) {
                        cursor = mb_start;
                        break;
                    }

                    self.current_block = 3; // Y4
                    if !Self::rl_decode_block(&self.in_fifo, &mut cursor, &mut y4_blk, &self.luma_quant) {
                        cursor = mb_start;
                        break;
                    }

                    self.real_idct(&mut cr_blk);
                    self.real_idct(&mut cb_blk);
                    self.real_idct(&mut y1_blk);
                    self.real_idct(&mut y2_blk);
                    self.real_idct(&mut y3_blk);
                    self.real_idct(&mut y4_blk);

                    self.current_block = 4;
                    self.yuv_to_rgb(&cr_blk, &cb_blk, &y1_blk, &y2_blk, &y3_blk, &y4_blk);
                }
            }
        }

        if cursor > 0 {
            self.in_fifo.drain(0..cursor);
        }
    }

    fn rl_decode_block(stream: &[u16], cursor: &mut usize, blk: &mut [i16; 64], qt: &[u8; 64]) -> bool {
        blk.fill(0);

        // Pula códigos de padding 0xFE00 entre blocos
        while *cursor < stream.len() && stream[*cursor] == 0xFE00 {
            *cursor += 1;
        }
        if *cursor >= stream.len() {
            return false;
        }

        let mut n = stream[*cursor];
        *cursor += 1;

        let q_scale = ((n >> 10) & 0x3F) as i32;
        let dc = sign_extend_10(n & 0x3FF);

        let mut val = if q_scale == 0 {
            dc * 2
        } else {
            dc * (qt[0] as i32)
        };
        val = val.clamp(-0x400, 0x3FF);

        if q_scale == 0 {
            blk[0] = val as i16;
        } else {
            blk[ZIGZAG[0]] = val as i16;
        }

        let mut k = 0usize;
        let mut found_eob = false;

        while *cursor < stream.len() {
            n = stream[*cursor];
            *cursor += 1;

            if n == 0xFE00 {
                found_eob = true;
                break;
            }

            let skip = ((n >> 10) & 0x3F) as usize;
            k += skip + 1;

            if k < 64 {
                let ac = sign_extend_10(n & 0x3FF);
                val = if q_scale == 0 {
                    ac * 2
                } else {
                    (ac * (qt[k] as i32) * q_scale + 4) >> 3
                };
                val = val.clamp(-0x400, 0x3FF);

                if q_scale == 0 {
                    blk[k] = val as i16;
                } else {
                    blk[ZIGZAG[k]] = val as i16;
                }
            }
        }

        found_eob
    }

    /// IDCT 2D com precisão total de 64 bits em ponto fixo oficial (duas passagens de 8x8)
    fn real_idct(&self, blk: &mut [i16; 64]) {
        let mut temp = [0i64; 64];

        // Passagem 1: Colunas para buffer temporário 64 bits
        for x in 0..8 {
            for y in 0..8 {
                let mut sum = 0i64;
                for u in 0..8 {
                    sum += (blk[u * 8 + x] as i64) * (self.idct_table[u * 8 + y] as i64);
                }
                temp[x + y * 8] = sum;
            }
        }

        // Passagem 2: Linhas do buffer temporário de volta para blk com arredondamento e saturação
        for x in 0..8 {
            for y in 0..8 {
                let mut sum = 0i64;
                for u in 0..8 {
                    sum += temp[u + y * 8] * (self.idct_table[u * 8 + x] as i64);
                }
                let rounded = (sum >> 32) + ((sum >> 31) & 1);
                let val = (((rounded as i32) << 23) >> 23).clamp(-128, 127);
                blk[x + y * 8] = val as i16;
            }
        }
    }

    fn y_to_mono(&mut self, y_blk: &[i16; 64]) {
        let mut bytes = [0u8; 64];
        for (i, &y_val) in y_blk.iter().enumerate() {
            let mut y = y_val.clamp(-128, 127);
            if !self.output_signed {
                y += 128;
            }
            bytes[i] = y as u8;
        }

        match self.output_depth {
            OutputDepth::Bpp4 => {
                for chunk in bytes.chunks(8) {
                    let mut word = 0u32;
                    for (i, &b) in chunk.iter().enumerate() {
                        word |= (((b >> 4) & 0x0F) as u32) << (i * 4);
                    }
                    self.out_fifo.push(word);
                }
            }
            OutputDepth::Bpp8 => {
                for chunk in bytes.chunks(4) {
                    self.out_fifo.push(u32::from_le_bytes(chunk.try_into().unwrap()));
                }
            }
            _ => {}
        }
    }

    fn yuv_to_rgb(
        &mut self,
        cr_blk: &[i16; 64],
        cb_blk: &[i16; 64],
        y1_blk: &[i16; 64],
        y2_blk: &[i16; 64],
        y3_blk: &[i16; 64],
        y4_blk: &[i16; 64],
    ) {
        let mask_bit = if self.output_bit15 { 0x8000u16 } else { 0 };

        match self.output_depth {
            OutputDepth::Bpp24 => {
                // Buffer em stack de 768 bytes (16x16 pixels * 3 bytes) - Zero alocações de heap
                let mut rgb_bytes = [0u8; 16 * 16 * 3];
                let mut byte_idx = 0;

                for py in 0..16 {
                    for px in 0..16 {
                        let cr = cr_blk[(px >> 1) + (py >> 1) * 8] as i32;
                        let cb = cb_blk[(px >> 1) + (py >> 1) * 8] as i32;

                        let y_blk = match (px >= 8, py >= 8) {
                            (false, false) => y1_blk,
                            (true, false) => y2_blk,
                            (false, true) => y3_blk,
                            (true, true) => y4_blk,
                        };
                        let luma = y_blk[(px & 7) + (py & 7) * 8] as i32;

                        // Coeficientes BT.601 oficiais com arredondamento preciso (+512)
                        let r_off = (1436 * cr + 512) >> 10;
                        let g_off = (-352 * cb - 731 * cr + 512) >> 10;
                        let b_off = (1815 * cb + 512) >> 10;

                        let mut r = (luma + r_off).clamp(-128, 127);
                        let mut g = (luma + g_off).clamp(-128, 127);
                        let mut b = (luma + b_off).clamp(-128, 127);

                        if !self.output_signed {
                            r += 128;
                            g += 128;
                            b += 128;
                        }

                        rgb_bytes[byte_idx] = r as u8;
                        rgb_bytes[byte_idx + 1] = g as u8;
                        rgb_bytes[byte_idx + 2] = b as u8;
                        byte_idx += 3;
                    }
                }

                for chunk in rgb_bytes.chunks_exact(4) {
                    self.out_fifo.push(u32::from_le_bytes(chunk.try_into().unwrap()));
                }
            }
            _ => {
                // Modo 15bpp (RGB555 com Bit 15 configurável) - Zero alocações de heap
                let mut rgb_15 = [0u16; 16 * 16];
                let mut pixel_idx = 0;

                for py in 0..16 {
                    for px in 0..16 {
                        let cr = cr_blk[(px >> 1) + (py >> 1) * 8] as i32;
                        let cb = cb_blk[(px >> 1) + (py >> 1) * 8] as i32;

                        let y_blk = match (px >= 8, py >= 8) {
                            (false, false) => y1_blk,
                            (true, false) => y2_blk,
                            (false, true) => y3_blk,
                            (true, true) => y4_blk,
                        };
                        let luma = y_blk[(px & 7) + (py & 7) * 8] as i32;

                        let r_off = (1436 * cr + 512) >> 10;
                        let g_off = (-352 * cb - 731 * cr + 512) >> 10;
                        let b_off = (1815 * cb + 512) >> 10;

                        let mut r = (luma + r_off).clamp(-128, 127);
                        let mut g = (luma + g_off).clamp(-128, 127);
                        let mut b = (luma + b_off).clamp(-128, 127);

                        if !self.output_signed {
                            r += 128;
                            g += 128;
                            b += 128;
                        }

                        let r5 = ((r as u16) >> 3) & 0x1F;
                        let g5 = ((g as u16) >> 3) & 0x1F;
                        let b5 = ((b as u16) >> 3) & 0x1F;
                        rgb_15[pixel_idx] = r5 | (g5 << 5) | (b5 << 10) | mask_bit;
                        pixel_idx += 1;
                    }
                }

                for pair in rgb_15.chunks_exact(2) {
                    let word = (pair[0] as u32) | ((pair[1] as u32) << 16);
                    self.out_fifo.push(word);
                }
            }
        }
    }
}

#[inline(always)]
fn sign_extend_10(val: u16) -> i32 {
    (((val & 0x3FF) as i16) << 6 >> 6) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mdec_status_and_reset() {
        let mut mdec = Mdec::new();
        let stat = mdec.read_status();
        assert_eq!(stat & 0xFFFF, 0xFFFF);
        assert_ne!(stat & (1 << 31), 0);

        mdec.write_control(1 << 31);
        assert_eq!(mdec.words_remaining, 0);
    }

    #[test]
    fn test_mdec_quantization_table_load() {
        let mut mdec = Mdec::new();
        mdec.write_command(0x4000_0000);
        assert_eq!(mdec.words_remaining, 16);

        mdec.write_command(0x0403_0201);
        assert_eq!(mdec.luma_quant[0], 0x01);
        assert_eq!(mdec.luma_quant[1], 0x02);
        assert_eq!(mdec.luma_quant[2], 0x03);
        assert_eq!(mdec.luma_quant[3], 0x04);
        assert_eq!(mdec.words_remaining, 15);
    }

    #[test]
    fn test_mdec_idct_dc_accuracy() {
        let mut mdec = Mdec::new();
        mdec.write_control((1 << 30) | (1 << 29));

        // Testa um macrobloco 15bpp com DC neutro (Cr=0, Cb=0, Y=0 -> RGB cinza médio = (128, 128, 128) -> RGB15 = (16, 16, 16) = 0x4210)
        let cmd = (1 << 29) | (3 << 27) | (1 << 25) | 6;
        mdec.write_command(cmd);

        for _ in 0..6 {
            // q_scale = 1 (1 << 10 = 0x0400), DC = 0, seguido por EOB (0xFE00)
            mdec.write_command(0xFE00_0400);
        }

        assert_eq!(mdec.out_fifo.len(), 128);
        // Cada pixel de 16 bits deve ter Bit 15 = 1 (0x8000) + R=16, G=16, B=16 (0x4210) = 0xC210!
        let first_word = mdec.read_data();
        assert_eq!(first_word & 0xFFFF, 0xC210);
        assert_eq!(first_word >> 16, 0xC210);
    }
}
