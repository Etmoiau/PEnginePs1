// src/gpu.rs

pub const VRAM_WIDTH: usize = 1024;
pub const VRAM_HEIGHT: usize = 512;
pub const TV_WIDTH: usize = 320;
pub const TV_HEIGHT: usize = 240;

const DITHER_TABLE: [[i16; 4]; 4] = [
    [-4,  0, -3,  1],
    [ 2, -2,  3, -1],
    [-3,  1, -4,  0],
    [ 3, -1,  2, -2],
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderMode {
    Software,
    Hardware,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InternalResolution {
    Native1x = 1,
    Hd2x = 2,
    FullHd4x = 4,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShaderEffect {
    None,
    Bilinear,
    CrtScanlines,
    BilinearCrt,
    RfNoise,
}

#[derive(Clone, Copy, Debug)]
pub struct GpuConfig {
    pub render_mode: RenderMode,
    pub bilinear_filter: bool,
    pub perspective_correct: bool,
    pub pgxp_enabled: bool,
    pub shader: ShaderEffect,
    pub internal_res: InternalResolution,
    pub widescreen_hack: bool,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            render_mode: RenderMode::Hardware,
            bilinear_filter: false,
            perspective_correct: false,
            pgxp_enabled: false,
            shader: ShaderEffect::None,
            internal_res: InternalResolution::Native1x,
            widescreen_hack: false,
        }
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Vertex {
    pub x: i32,
    pub y: i32,
    pub fx: f32,
    pub fy: f32,
    pub w: f32,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub u: u16,
    pub v: u16,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum TransferMode {
    None,
    CpuToVram,
    VramToCpu,
}

pub struct Gpu {
    pub vram: Box<[u16; VRAM_WIDTH * VRAM_HEIGHT]>,
    pub config: GpuConfig,
    pub status: u32,
    pub read_word: u32,

    pub draw_area_left: i32,
    pub draw_area_top: i32,
    pub draw_area_right: i32,
    pub draw_area_bottom: i32,

    pub draw_offset_x: i32,
    pub draw_offset_y: i32,

    pub page_base_x: usize,
    pub page_base_y: usize,
    pub semi_transparency_mode: u8,
    pub texture_depth: u8,
    pub dither: bool,
    pub draw_to_display: bool,
    pub texture_disable: bool,
    pub tex_rect_x_flip: bool,
    pub tex_rect_y_flip: bool,

    pub tex_window_mask_x: u8,
    pub tex_window_mask_y: u8,
    pub tex_window_offset_x: u8,
    pub tex_window_offset_y: u8,

    pub clut_x: usize,
    pub clut_y: usize,

    pub set_mask_bit: bool,
    pub check_mask_bit: bool,

    pub display_disabled: bool,
    pub display_vram_x: usize,
    pub display_vram_y: usize,
    pub display_horiz_start: u16,
    pub display_horiz_end: u16,
    pub display_line_start: u16,
    pub display_line_end: u16,
    pub display_depth_24: bool,
    pub interlace: bool,
    pub field: bool,
    pub show_full_vram: bool, // Alterna entre Modo TV (4:3) e Modo Debug VRAM (1024x512)

    gp0_buffer: Vec<u32>,
    gp0_words_remaining: u32,
    in_polyline: bool,
    polyline_shaded: bool,

    transfer_mode: TransferMode,
    trans_x: usize,
    trans_y: usize,
    trans_w: usize,
    trans_h: usize,
    trans_cur_x: usize,
    trans_cur_y: usize,
    trans_words_remaining: u32,
    /// Perfilamento (modo --bench): tempo acumulado executando comandos GP0
    pub perf_enabled: bool,
    pub perf_gp0_ns: u64,
}

impl Gpu {
    pub fn new() -> Self {
        Self {
            vram: vec![0u16; VRAM_WIDTH * VRAM_HEIGHT]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            config: GpuConfig::default(),
            status: 0x1480_2000,
            read_word: 0,
            draw_area_left: 0,
            draw_area_top: 0,
            draw_area_right: 1023,
            draw_area_bottom: 511,
            draw_offset_x: 0,
            draw_offset_y: 0,
            page_base_x: 0,
            page_base_y: 0,
            semi_transparency_mode: 0,
            texture_depth: 0,
            dither: false,
            draw_to_display: false,
            texture_disable: false,
            tex_rect_x_flip: false,
            tex_rect_y_flip: false,
            tex_window_mask_x: 0,
            tex_window_mask_y: 0,
            tex_window_offset_x: 0,
            tex_window_offset_y: 0,
            clut_x: 0,
            clut_y: 0,
            set_mask_bit: false,
            check_mask_bit: false,
            display_disabled: false,
            display_vram_x: 0,
            display_vram_y: 0,
            display_horiz_start: 0x200,
            display_horiz_end: 0xC00,
            display_line_start: 0x10,
            display_line_end: 0x100,
            display_depth_24: false,
            interlace: false,
            field: true,
            show_full_vram: false,
            gp0_buffer: Vec::with_capacity(64),
            gp0_words_remaining: 0,
            in_polyline: false,
            polyline_shaded: false,
            transfer_mode: TransferMode::None,
            trans_x: 0,
            trans_y: 0,
            trans_w: 0,
            trans_h: 0,
            trans_cur_x: 0,
            trans_cur_y: 0,
            trans_words_remaining: 0,
            perf_enabled: false,
            perf_gp0_ns: 0,
        }
    }

    #[inline(always)]
    pub fn read_vram(&self, x: usize, y: usize) -> u16 {
        self.vram[(y & 0x1FF) * VRAM_WIDTH + (x & 0x3FF)]
    }

    #[inline(always)]
    pub fn write_vram(&mut self, x: usize, y: usize, color: u16) {
        let x = x & 0x3FF;
        let y = y & 0x1FF;
        let idx = y * VRAM_WIDTH + x;

        if self.check_mask_bit && (self.vram[idx] & 0x8000) != 0 {
            return;
        }

        let mask = if self.set_mask_bit { 0x8000 } else { 0x0000 };
        self.vram[idx] = color | mask;
    }

    pub fn read_status(&self) -> u32 {
        let mut r = self.status & 0x7F00_0000;
        r |= (self.page_base_x as u32 / 64) & 0x0F;
        r |= ((self.page_base_y as u32 / 256) & 1) << 4;
        r |= (self.semi_transparency_mode as u32 & 3) << 5;
        r |= (self.texture_depth as u32 & 3) << 7;
        r |= (self.dither as u32) << 9;
        r |= (self.draw_to_display as u32) << 10;
        r |= (self.set_mask_bit as u32) << 11;
        r |= (self.check_mask_bit as u32) << 12;
        r |= (self.field as u32) << 13;
        r |= (self.texture_disable as u32) << 15;
        r |= (self.display_depth_24 as u32) << 21;
        r |= (self.interlace as u32) << 22;
        r |= (self.display_disabled as u32) << 23;

        let dma_dir = (self.status >> 29) & 3;
        let dma_req = match dma_dir {
            0 => 0,
            _ => 1,
        };
        r |= dma_req << 25;
        r |= 1 << 26;
        r |= 1 << 27;
        r |= 1 << 28;
        r |= ((self.status >> 31) & 1) << 31;
        r
    }

    pub fn read_data(&mut self) -> u32 {
        if self.transfer_mode == TransferMode::VramToCpu && self.trans_words_remaining > 0 {
            let p0 = self.read_vram(self.trans_cur_x, self.trans_cur_y);
            self.advance_transfer();
            let p1 = self.read_vram(self.trans_cur_x, self.trans_cur_y);
            self.advance_transfer();

            self.trans_words_remaining -= 1;
            if self.trans_words_remaining == 0 {
                self.transfer_mode = TransferMode::None;
            }
            return (p0 as u32) | ((p1 as u32) << 16);
        }
        self.read_word
    }

    pub fn write_gp0(&mut self, val: u32) {
        if self.transfer_mode == TransferMode::CpuToVram {
            let p0 = val as u16;
            let p1 = (val >> 16) as u16;

            if self.trans_cur_y < self.trans_y + self.trans_h {
                self.write_vram(self.trans_cur_x, self.trans_cur_y, p0);
            }
            self.advance_transfer();
            if self.trans_cur_y < self.trans_y + self.trans_h {
                self.write_vram(self.trans_cur_x, self.trans_cur_y, p1);
            }
            self.advance_transfer();

            self.trans_words_remaining = self.trans_words_remaining.saturating_sub(1);
            if self.trans_words_remaining == 0 {
                self.transfer_mode = TransferMode::None;
            }
            return;
        }

        if self.in_polyline {
            self.gp0_buffer.push(val);
            if (val & 0xF000_F000) == 0x5000_5000 {
                self.in_polyline = false;
                self.execute_polyline();
                self.gp0_buffer.clear();
            }
            return;
        }

        if self.gp0_buffer.is_empty() {
            let opcode = val >> 24;
            match opcode {
                0x48..=0x4F => {
                    self.in_polyline = true;
                    self.polyline_shaded = false;
                    self.gp0_buffer.push(val);
                    return;
                }
                0x58..=0x5F => {
                    self.in_polyline = true;
                    self.polyline_shaded = true;
                    self.gp0_buffer.push(val);
                    return;
                }
                _ => {}
            }

            self.gp0_words_remaining = match opcode {
                0x00 | 0x01 => 1,
                0x02 => 3,
                0x20..=0x23 => 4,
                0x24..=0x27 => 7,
                0x28..=0x2B => 5,
                0x2C..=0x2F => 9,
                0x30..=0x33 => 6,
                0x34..=0x37 => 9,
                0x38..=0x3B => 8,
                0x3C..=0x3F => 12,
                0x40..=0x47 => 3,
                0x50..=0x57 => 4,
                0x60..=0x63 => 3,
                0x64..=0x67 => 4,
                0x68..=0x6B => 2,
                0x6C..=0x6F => 3,
                0x70..=0x73 => 2,
                0x74..=0x77 => 3,
                0x78..=0x7B => 2,
                0x7C..=0x7F => 3,
                0x80..=0x9F => 4,
                0xA0..=0xBF => 3,
                0xC0..=0xDF => 3,
                0xE1..=0xE6 => 1,
                _ => 1,
            };
        }

        self.gp0_buffer.push(val);
        self.gp0_words_remaining -= 1;

        if self.gp0_words_remaining == 0 {
            if self.perf_enabled {
                let t0 = std::time::Instant::now();
                self.execute_gp0();
                self.perf_gp0_ns += t0.elapsed().as_nanos() as u64;
            } else {
                self.execute_gp0();
            }
            self.gp0_buffer.clear();
        }
    }

    pub fn write_gp1(&mut self, val: u32) {
        let opcode = val >> 24;
        match opcode {
            0x00 => {
                self.status = 0x1480_2000;
                self.gp0_buffer.clear();
                self.gp0_words_remaining = 0;
                self.in_polyline = false;
                self.transfer_mode = TransferMode::None;
                self.page_base_x = 0;
                self.page_base_y = 0;
                self.semi_transparency_mode = 0;
                self.texture_depth = 0;
                self.dither = false;
                self.draw_to_display = false;
                self.texture_disable = false;
                self.tex_rect_x_flip = false;
                self.tex_rect_y_flip = false;
                self.tex_window_mask_x = 0;
                self.tex_window_mask_y = 0;
                self.tex_window_offset_x = 0;
                self.tex_window_offset_y = 0;
                self.draw_area_left = 0;
                self.draw_area_top = 0;
                self.draw_area_right = 1023;
                self.draw_area_bottom = 511;
                self.draw_offset_x = 0;
                self.draw_offset_y = 0;
                self.set_mask_bit = false;
                self.check_mask_bit = false;
                self.display_disabled = true;
                self.display_vram_x = 0;
                self.display_vram_y = 0;
                self.display_horiz_start = 0x200;
                self.display_horiz_end = 0xC00;
                self.display_line_start = 0x10;
                self.display_line_end = 0x100;
                self.display_depth_24 = false;
                self.interlace = true;
                self.field = true;
            }
            0x01 => {
                self.gp0_buffer.clear();
                self.gp0_words_remaining = 0;
                self.in_polyline = false;
                self.transfer_mode = TransferMode::None;
            }
            0x02 => {
                self.status &= !(1 << 24);
            }
            0x03 => {
                self.display_disabled = (val & 1) != 0;
            }
            0x04 => {
                self.status &= !(3 << 29);
                self.status |= (val & 3) << 29;
            }
            0x05 => {
                self.display_vram_x = (val & 0x3FE) as usize;
                self.display_vram_y = ((val >> 10) & 0x1FF) as usize;
            }
            0x06 => {
                self.display_horiz_start = (val & 0xFFF) as u16;
                self.display_horiz_end = ((val >> 12) & 0xFFF) as u16;
            }
            0x07 => {
                self.display_line_start = (val & 0x3FF) as u16;
                self.display_line_end = ((val >> 10) & 0x3FF) as u16;
            }
            0x08 => {
                let hr1 = (val & 3) << 17;
                let hr2 = ((val >> 6) & 1) << 16;
                let vres = ((val >> 2) & 1) << 19;
                let vmode = ((val >> 3) & 1) << 20;
                let is_24 = (val & (1 << 4)) != 0;
                let interlace = (val & (1 << 5)) != 0;
                self.status = (self.status & !0x007F_0000)
                    | hr1
                    | hr2
                    | vres
                    | vmode
                    | (if is_24 { 1 << 21 } else { 0 })
                    | (if interlace { 1 << 22 } else { 0 });
                self.display_depth_24 = is_24;
                self.interlace = interlace;
                self.field = true;
            }
            0x10..=0x1F => {
                self.read_word = match val & 0x07 {
                    0x02 => {
                        (self.tex_window_mask_x as u32)
                            | ((self.tex_window_mask_y as u32) << 5)
                            | ((self.tex_window_offset_x as u32) << 10)
                            | ((self.tex_window_offset_y as u32) << 15)
                    }
                    0x03 => (self.draw_area_left as u32) | ((self.draw_area_top as u32) << 10),
                    0x04 => (self.draw_area_right as u32) | ((self.draw_area_bottom as u32) << 10),
                    0x05 => {
                        ((self.draw_offset_x as u32) & 0x7FF)
                            | (((self.draw_offset_y as u32) & 0x7FF) << 11)
                    }
                    0x07 => 2,
                    _ => self.read_word,
                };
            }
            _ => {}
        }
    }

    fn execute_polyline(&mut self) {
        let cmd = self.gp0_buffer[0];
        let semi = (cmd & (1 << 25)) != 0;

        if self.polyline_shaded {
            let mut idx = 0;
            while idx + 3 < self.gp0_buffer.len() - 1 {
                let v0 = self.decode_vertex(self.gp0_buffer[idx + 1], self.gp0_buffer[idx], 0);
                let v1 = self.decode_vertex(self.gp0_buffer[idx + 3], self.gp0_buffer[idx + 2], 0);
                self.rasterize_line(v0, v1, semi, true);
                idx += 2;
            }
        } else {
            let mut idx = 1;
            while idx + 1 < self.gp0_buffer.len() - 1 {
                let v0 = self.decode_vertex(self.gp0_buffer[idx], cmd, 0);
                let v1 = self.decode_vertex(self.gp0_buffer[idx + 1], cmd, 0);
                self.rasterize_line(v0, v1, semi, false);
                idx += 1;
            }
        }
    }

    fn execute_gp0(&mut self) {
        let cmd = self.gp0_buffer[0];
        let opcode = cmd >> 24;

        match opcode {
            0x00 | 0x01 => {}
            0x02 => {
                let color16 = bgr24_to_rgb15(cmd);
                let x = (self.gp0_buffer[1] & 0x3F0) as usize;
                let y = ((self.gp0_buffer[1] >> 16) & 0x1FF) as usize;
                let w = (((self.gp0_buffer[2] & 0x3FF) + 0x0F) & !0x0F) as usize;
                let h = ((self.gp0_buffer[2] >> 16) & 0x1FF) as usize;

                for row in 0..h {
                    let py = (y + row) & 0x1FF;
                    for col in 0..w {
                        let px = (x + col) & 0x3FF;
                        self.vram[py * VRAM_WIDTH + px] = color16;
                    }
                }
            }
            0x20..=0x23 => {
                let semi = (cmd & (1 << 25)) != 0;
                let v0 = self.decode_vertex(self.gp0_buffer[1], cmd, 0);
                let v1 = self.decode_vertex(self.gp0_buffer[2], cmd, 0);
                let v2 = self.decode_vertex(self.gp0_buffer[3], cmd, 0);
                self.rasterize_triangle(v0, v1, v2, false, false, semi, false);
            }
            0x24..=0x27 => {
                let raw_tex = (cmd & (1 << 24)) != 0;
                let semi = (cmd & (1 << 25)) != 0;
                self.decode_clut(self.gp0_buffer[2] >> 16);
                self.decode_tpage(self.gp0_buffer[4] >> 16);

                let v0 = self.decode_vertex(self.gp0_buffer[1], cmd, self.gp0_buffer[2]);
                let v1 = self.decode_vertex(self.gp0_buffer[3], cmd, self.gp0_buffer[4]);
                let v2 = self.decode_vertex(self.gp0_buffer[5], cmd, self.gp0_buffer[6]);
                self.rasterize_triangle(v0, v1, v2, true, raw_tex, semi, !raw_tex && self.dither);
            }
            0x28..=0x2B => {
                let semi = (cmd & (1 << 25)) != 0;
                let v0 = self.decode_vertex(self.gp0_buffer[1], cmd, 0);
                let v1 = self.decode_vertex(self.gp0_buffer[2], cmd, 0);
                let v2 = self.decode_vertex(self.gp0_buffer[3], cmd, 0);
                let v3 = self.decode_vertex(self.gp0_buffer[4], cmd, 0);
                self.rasterize_triangle(v0, v1, v2, false, false, semi, false);
                self.rasterize_triangle(v1, v3, v2, false, false, semi, false);
            }
            0x2C..=0x2F => {
                let raw_tex = (cmd & (1 << 24)) != 0;
                let semi = (cmd & (1 << 25)) != 0;
                self.decode_clut(self.gp0_buffer[2] >> 16);
                self.decode_tpage(self.gp0_buffer[4] >> 16);

                let v0 = self.decode_vertex(self.gp0_buffer[1], cmd, self.gp0_buffer[2]);
                let v1 = self.decode_vertex(self.gp0_buffer[3], cmd, self.gp0_buffer[4]);
                let v2 = self.decode_vertex(self.gp0_buffer[5], cmd, self.gp0_buffer[6]);
                let v3 = self.decode_vertex(self.gp0_buffer[7], cmd, self.gp0_buffer[8]);
                let do_dither = !raw_tex && self.dither;
                self.rasterize_triangle(v0, v1, v2, true, raw_tex, semi, do_dither);
                self.rasterize_triangle(v1, v3, v2, true, raw_tex, semi, do_dither);
            }
            0x30..=0x33 => {
                let semi = (cmd & (1 << 25)) != 0;
                let v0 = self.decode_vertex(self.gp0_buffer[1], self.gp0_buffer[0], 0);
                let v1 = self.decode_vertex(self.gp0_buffer[3], self.gp0_buffer[2], 0);
                let v2 = self.decode_vertex(self.gp0_buffer[5], self.gp0_buffer[4], 0);
                self.rasterize_triangle(v0, v1, v2, false, false, semi, self.dither);
            }
            0x34..=0x37 => {
                let raw_tex = (cmd & (1 << 24)) != 0;
                let semi = (cmd & (1 << 25)) != 0;
                self.decode_clut(self.gp0_buffer[2] >> 16);
                self.decode_tpage(self.gp0_buffer[5] >> 16);

                let v0 = self.decode_vertex(self.gp0_buffer[1], self.gp0_buffer[0], self.gp0_buffer[2]);
                let v1 = self.decode_vertex(self.gp0_buffer[4], self.gp0_buffer[3], self.gp0_buffer[5]);
                let v2 = self.decode_vertex(self.gp0_buffer[7], self.gp0_buffer[6], self.gp0_buffer[8]);
                self.rasterize_triangle(v0, v1, v2, true, raw_tex, semi, self.dither);
            }
            0x38..=0x3B => {
                let semi = (cmd & (1 << 25)) != 0;
                let v0 = self.decode_vertex(self.gp0_buffer[1], self.gp0_buffer[0], 0);
                let v1 = self.decode_vertex(self.gp0_buffer[3], self.gp0_buffer[2], 0);
                let v2 = self.decode_vertex(self.gp0_buffer[5], self.gp0_buffer[4], 0);
                let v3 = self.decode_vertex(self.gp0_buffer[7], self.gp0_buffer[6], 0);
                self.rasterize_triangle(v0, v1, v2, false, false, semi, self.dither);
                self.rasterize_triangle(v1, v3, v2, false, false, semi, self.dither);
            }
            0x3C..=0x3F => {
                let raw_tex = (cmd & (1 << 24)) != 0;
                let semi = (cmd & (1 << 25)) != 0;
                self.decode_clut(self.gp0_buffer[2] >> 16);
                self.decode_tpage(self.gp0_buffer[5] >> 16);

                let v0 = self.decode_vertex(self.gp0_buffer[1], self.gp0_buffer[0], self.gp0_buffer[2]);
                let v1 = self.decode_vertex(self.gp0_buffer[4], self.gp0_buffer[3], self.gp0_buffer[5]);
                let v2 = self.decode_vertex(self.gp0_buffer[7], self.gp0_buffer[6], self.gp0_buffer[8]);
                let v3 = self.decode_vertex(self.gp0_buffer[10], self.gp0_buffer[9], self.gp0_buffer[11]);
                self.rasterize_triangle(v0, v1, v2, true, raw_tex, semi, self.dither);
                self.rasterize_triangle(v1, v3, v2, true, raw_tex, semi, self.dither);
            }
            0x40..=0x47 => {
                let semi = (cmd & (1 << 25)) != 0;
                let v0 = self.decode_vertex(self.gp0_buffer[1], cmd, 0);
                let v1 = self.decode_vertex(self.gp0_buffer[2], cmd, 0);
                self.rasterize_line(v0, v1, semi, false);
            }
            0x50..=0x57 => {
                let semi = (cmd & (1 << 25)) != 0;
                let v0 = self.decode_vertex(self.gp0_buffer[1], self.gp0_buffer[0], 0);
                let v1 = self.decode_vertex(self.gp0_buffer[3], self.gp0_buffer[2], 0);
                self.rasterize_line(v0, v1, semi, true);
            }
            0x60..=0x7F => {
                let textured = (cmd & (1 << 26)) != 0;
                let raw_tex = (cmd & (1 << 24)) != 0;
                let semi = (cmd & (1 << 25)) != 0;

                let (w, h) = match (cmd >> 27) & 3 {
                    1 => (1, 1),
                    2 => (8, 8),
                    3 => (16, 16),
                    _ => {
                        let sz = self.gp0_buffer[if textured { 3 } else { 2 }];
                        ((sz & 0x3FF) as i32, ((sz >> 16) & 0x1FF) as i32)
                    }
                };

                let pos = self.gp0_buffer[1];
                let x0 = sign_extend_11(pos & 0x7FF) + self.draw_offset_x;
                let y0 = sign_extend_11((pos >> 16) & 0x7FF) + self.draw_offset_y;

                if textured && !self.texture_disable {
                    self.decode_clut(self.gp0_buffer[2] >> 16);
                    let uv = self.gp0_buffer[2];
                    let u0 = (uv & 0xFF) as i32;
                    let v0 = ((uv >> 8) & 0xFF) as i32;

                    for dy in 0..h {
                        let y = y0 + dy;
                        if y < self.draw_area_top || y > self.draw_area_bottom {
                            continue;
                        }
                        let ty = if self.tex_rect_y_flip {
                            (v0 - dy) & 0xFF
                        } else {
                            (v0 + dy) & 0xFF
                        } as u16;

                        for dx in 0..w {
                            let x = x0 + dx;
                            if x < self.draw_area_left || x > self.draw_area_right {
                                continue;
                            }
                            let tx = if self.tex_rect_x_flip {
                                (u0 - dx) & 0xFF
                            } else {
                                (u0 + dx) & 0xFF
                            } as u16;

                            let tex = self.sample_texture(tx, ty);
                            if tex == 0 {
                                continue;
                            }

                            let stp_bit = tex & 0x8000;
                            let tr = (tex & 0x1F) as u32;
                            let tg = ((tex >> 5) & 0x1F) as u32;
                            let tb = ((tex >> 10) & 0x1F) as u32;

                            let mut col = if raw_tex {
                                (tr) | (tg << 5) | (tb << 10)
                            } else {
                                let cr = (cmd & 0xFF) as u32;
                                let cg = ((cmd >> 8) & 0xFF) as u32;
                                let cb = ((cmd >> 16) & 0xFF) as u32;

                                let r = ((tr * cr) >> 7).min(31);
                                let g = ((tg * cg) >> 7).min(31);
                                let b = ((tb * cb) >> 7).min(31);
                                (r) | (g << 5) | (b << 10)
                            } as u16;

                            if semi && stp_bit != 0 {
                                let bg = self.read_vram(x as usize, y as usize);
                                col = self.blend(col, bg);
                            }
                            self.write_vram(x as usize, y as usize, col | stp_bit);
                        }
                    }
                } else {
                    let color16 = bgr24_to_rgb15(cmd);
                    for dy in 0..h {
                        let y = y0 + dy;
                        if y < self.draw_area_top || y > self.draw_area_bottom {
                            continue;
                        }
                        for dx in 0..w {
                            let x = x0 + dx;
                            if x < self.draw_area_left || x > self.draw_area_right {
                                continue;
                            }
                            let final_col = if semi {
                                let bg = self.read_vram(x as usize, y as usize);
                                self.blend(color16, bg)
                            } else {
                                color16
                            };
                            self.write_vram(x as usize, y as usize, final_col);
                        }
                    }
                }
            }
            0x80..=0x9F => {
                let src = self.gp0_buffer[1];
                let dst = self.gp0_buffer[2];
                let sz = self.gp0_buffer[3];

                let src_x = (src & 0x3FF) as usize;
                let src_y = ((src >> 16) & 0x1FF) as usize;
                let dst_x = (dst & 0x3FF) as usize;
                let dst_y = ((dst >> 16) & 0x1FF) as usize;
                let w = ((sz & 0xFFFF).wrapping_sub(1) & 0x3FF) as usize + 1;
                let h = (((sz >> 16) & 0xFFFF).wrapping_sub(1) & 0x1FF) as usize + 1;

                // Cópia direta sem alocar Vec temporário, respeitando direção de sobreposição
                let y_rev = dst_y > src_y;
                let x_rev = dst_x > src_x;

                for step_y in 0..h {
                    let dy = if y_rev { h - 1 - step_y } else { step_y };
                    for step_x in 0..w {
                        let dx = if x_rev { w - 1 - step_x } else { step_x };
                        let pixel = self.read_vram(src_x + dx, src_y + dy);
                        self.write_vram(dst_x + dx, dst_y + dy, pixel);
                    }
                }
            }
            0xA0..=0xBF => {
                self.setup_transfer(TransferMode::CpuToVram);
            }
            0xC0..=0xDF => {
                self.setup_transfer(TransferMode::VramToCpu);
            }
            0xE1 => {
                self.page_base_x = ((cmd & 0x0F) * 64) as usize;
                self.page_base_y = (((cmd >> 4) & 1) * 256) as usize;
                self.semi_transparency_mode = ((cmd >> 5) & 3) as u8;
                self.texture_depth = ((cmd >> 7) & 3) as u8;
                self.dither = ((cmd >> 9) & 1) != 0;
                self.draw_to_display = ((cmd >> 10) & 1) != 0;
                self.texture_disable = ((cmd >> 11) & 1) != 0;
                self.tex_rect_x_flip = ((cmd >> 12) & 1) != 0;
                self.tex_rect_y_flip = ((cmd >> 13) & 1) != 0;
            }
            0xE2 => {
                self.tex_window_mask_x = (cmd & 0x1F) as u8;
                self.tex_window_mask_y = ((cmd >> 5) & 0x1F) as u8;
                self.tex_window_offset_x = ((cmd >> 10) & 0x1F) as u8;
                self.tex_window_offset_y = ((cmd >> 15) & 0x1F) as u8;
            }
            0xE3 => {
                self.draw_area_left = (cmd & 0x3FF) as i32;
                self.draw_area_top = ((cmd >> 10) & 0x1FF) as i32;
            }
            0xE4 => {
                self.draw_area_right = (cmd & 0x3FF) as i32;
                self.draw_area_bottom = ((cmd >> 10) & 0x1FF) as i32;
            }
            0xE5 => {
                self.draw_offset_x = sign_extend_11(cmd & 0x7FF);
                self.draw_offset_y = sign_extend_11((cmd >> 11) & 0x7FF);
            }
            0xE6 => {
                self.set_mask_bit = (cmd & 1) != 0;
                self.check_mask_bit = (cmd & 2) != 0;
            }
            _ => {}
        }
    }

    #[inline(always)]
    fn is_top_left(x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
        (y0 == y1 && x0 > x1) || (y1 < y0)
    }

    #[allow(clippy::too_many_arguments)]
    fn rasterize_triangle_software(
        &mut self,
        v0: Vertex,
        v1: Vertex,
        v2: Vertex,
        textured: bool,
        raw_tex: bool,
        semi_transparent: bool,
        apply_dither: bool,
    ) {
        let raw_min_x = v0.x.min(v1.x).min(v2.x);
        let raw_max_x = v0.x.max(v1.x).max(v2.x);
        let raw_min_y = v0.y.min(v1.y).min(v2.y);
        let raw_max_y = v0.y.max(v1.y).max(v2.y);

        if (raw_max_x - raw_min_x) >= 1024 || (raw_max_y - raw_min_y) >= 512 {
            return;
        }

        let min_x = raw_min_x.max(self.draw_area_left).max(0);
        let max_x = raw_max_x.min(self.draw_area_right).min((VRAM_WIDTH - 1) as i32);
        let min_y = raw_min_y.max(self.draw_area_top).max(0);
        let max_y = raw_max_y.min(self.draw_area_bottom).min((VRAM_HEIGHT - 1) as i32);

        if min_x > max_x || min_y > max_y {
            return;
        }

        let raw_area = edge_function(v0.x, v0.y, v1.x, v1.y, v2.x, v2.y);
        if raw_area == 0 {
            return;
        }
        let (v1, v2, area) = if raw_area < 0 {
            (v2, v1, (-raw_area) as i64)
        } else {
            (v1, v2, raw_area as i64)
        };

        // Recíproca com arredondamento fracionário exato
        let inv_area = (1i64 << 30) / area;
        let flat_color = v0.r == v1.r && v1.r == v2.r && v0.g == v1.g && v1.g == v2.g && v0.b == v1.b && v1.b == v2.b;

        // Regra Top-Left da Sony
        let tl0 = Self::is_top_left(v1.x, v1.y, v2.x, v2.y);
        let tl1 = Self::is_top_left(v2.x, v2.y, v0.x, v0.y);
        let tl2 = Self::is_top_left(v0.x, v0.y, v1.x, v1.y);

        let dw0_dx = (v2.y - v1.y) as i64;
        let dw1_dx = (v0.y - v2.y) as i64;
        let dw2_dx = (v1.y - v0.y) as i64;

        let dw0_dy = (v1.x - v2.x) as i64;
        let dw1_dy = (v2.x - v0.x) as i64;
        let dw2_dy = (v0.x - v1.x) as i64;

        let mut w0_row = edge_function(v1.x, v1.y, v2.x, v2.y, min_x, min_y) as i64;
        let mut w1_row = edge_function(v2.x, v2.y, v0.x, v0.y, min_x, min_y) as i64;
        let mut w2_row = edge_function(v0.x, v0.y, v1.x, v1.y, min_x, min_y) as i64;

        let do_tex = textured && !self.texture_disable;

        // Gradientes para interpolação linear rápida
        let (d_u_dx, d_u_dy, d_v_dx, d_v_dy) = if do_tex {
            (
                dw0_dx * v0.u as i64 + dw1_dx * v1.u as i64 + dw2_dx * v2.u as i64,
                dw0_dy * v0.u as i64 + dw1_dy * v1.u as i64 + dw2_dy * v2.u as i64,
                dw0_dx * v0.v as i64 + dw1_dx * v1.v as i64 + dw2_dx * v2.v as i64,
                dw0_dy * v0.v as i64 + dw1_dy * v1.v as i64 + dw2_dy * v2.v as i64,
            )
        } else {
            (0, 0, 0, 0)
        };

        let mut u_row_sum = if do_tex {
            w0_row * v0.u as i64 + w1_row * v1.u as i64 + w2_row * v2.u as i64
        } else {
            0
        };
        let mut v_row_sum = if do_tex {
            w0_row * v0.v as i64 + w1_row * v1.v as i64 + w2_row * v2.v as i64
        } else {
            0
        };

        let (d_r_dx, d_r_dy, d_g_dx, d_g_dy, d_b_dx, d_b_dy) = if !flat_color {
            (
                dw0_dx * v0.r as i64 + dw1_dx * v1.r as i64 + dw2_dx * v2.r as i64,
                dw0_dy * v0.r as i64 + dw1_dy * v1.r as i64 + dw2_dy * v2.r as i64,
                dw0_dx * v0.g as i64 + dw1_dx * v1.g as i64 + dw2_dx * v2.g as i64,
                dw0_dy * v0.g as i64 + dw1_dy * v1.g as i64 + dw2_dy * v2.g as i64,
                dw0_dx * v0.b as i64 + dw1_dx * v1.b as i64 + dw2_dx * v2.b as i64,
                dw0_dy * v0.b as i64 + dw1_dy * v1.b as i64 + dw2_dy * v2.b as i64,
            )
        } else {
            (0, 0, 0, 0, 0, 0)
        };

        let mut r_row_sum = if !flat_color {
            w0_row * v0.r as i64 + w1_row * v1.r as i64 + w2_row * v2.r as i64
        } else {
            0
        };
        let mut g_row_sum = if !flat_color {
            w0_row * v0.g as i64 + w1_row * v1.g as i64 + w2_row * v2.g as i64
        } else {
            0
        };
        let mut b_row_sum = if !flat_color {
            w0_row * v0.b as i64 + w1_row * v1.b as i64 + w2_row * v2.b as i64
        } else {
            0
        };

        // Cache da CLUT na stack (zero heap allocation, acesso instantâneo)
        let mut clut_cache = [0u16; 256];
        if do_tex {
            match self.texture_depth {
                0 => {
                    for i in 0..16 {
                        clut_cache[i] = self.read_vram(self.clut_x + i, self.clut_y);
                    }
                }
                1 => {
                    for i in 0..256 {
                        clut_cache[i] = self.read_vram(self.clut_x + i, self.clut_y);
                    }
                }
                _ => {}
            }
        }

        for y in min_y..=max_y {
            let mut w0 = w0_row;
            let mut w1 = w1_row;
            let mut w2 = w2_row;
            let mut u_sum = u_row_sum;
            let mut v_sum = v_row_sum;
            let mut r_sum = r_row_sum;
            let mut g_sum = g_row_sum;
            let mut b_sum = b_row_sum;

            let dither_row = &DITHER_TABLE[(y & 3) as usize];
            let mut entered = false;

            for x in min_x..=max_x {
                let inside0 = if tl0 { w0 >= 0 } else { w0 > 0 };
                let inside1 = if tl1 { w1 >= 0 } else { w1 > 0 };
                let inside2 = if tl2 { w2 >= 0 } else { w2 > 0 };

                if inside0 && inside1 && inside2 {
                    entered = true;
                    let (r, g, b) = if flat_color {
                        (v0.r as u32, v0.g as u32, v0.b as u32)
                    } else {
                        let cr = (((r_sum * inv_area) + (1 << 29)) >> 30).clamp(0, 255) as u32;
                        let cg = (((g_sum * inv_area) + (1 << 29)) >> 30).clamp(0, 255) as u32;
                        let cb = (((b_sum * inv_area) + (1 << 29)) >> 30).clamp(0, 255) as u32;
                        (cr, cg, cb)
                    };

                    let mut is_semi = semi_transparent;
                    let mut stp_bit = 0u16;

                    let (r5, g5, b5) = if do_tex {
                        let u = (((u_sum * inv_area) + (1 << 29)) >> 30).clamp(0, 255) as u16;
                        let v = (((v_sum * inv_area) + (1 << 29)) >> 30).clamp(0, 255) as u16;

                        let tex_color = self.sample_texture_fast(u, v, &clut_cache);
                        if tex_color == 0 {
                            w0 += dw0_dx;
                            w1 += dw1_dx;
                            w2 += dw2_dx;
                            u_sum += d_u_dx;
                            v_sum += d_v_dx;
                            r_sum += d_r_dx;
                            g_sum += d_g_dx;
                            b_sum += d_b_dx;
                            continue;
                        }

                        stp_bit = tex_color & 0x8000;
                        is_semi = semi_transparent && (stp_bit != 0);

                        let tr = (tex_color & 0x1F) as u32;
                        let tg = ((tex_color >> 5) & 0x1F) as u32;
                        let tb = ((tex_color >> 10) & 0x1F) as u32;

                        if raw_tex {
                            (tr, tg, tb)
                        } else {
                            let r_mod = (tr * r) >> 7;
                            let g_mod = (tg * g) >> 7;
                            let b_mod = (tb * b) >> 7;
                            (r_mod.min(31), g_mod.min(31), b_mod.min(31))
                        }
                    } else {
                        ((r >> 3) & 0x1F, (g >> 3) & 0x1F, (b >> 3) & 0x1F)
                    };

                    let (mut r_out, mut g_out, mut b_out) = (r5 as i16, g5 as i16, b5 as i16);

                    if apply_dither {
                        let d = dither_row[(x & 3) as usize];
                        r_out = ((r_out << 3) + d).clamp(0, 255) >> 3;
                        g_out = ((g_out << 3) + d).clamp(0, 255) >> 3;
                        b_out = ((b_out << 3) + d).clamp(0, 255) >> 3;
                    }

                    let mut final_col = (r_out as u16 & 0x1F)
                        | ((g_out as u16 & 0x1F) << 5)
                        | ((b_out as u16 & 0x1F) << 10);

                    if is_semi {
                        let bg = self.read_vram(x as usize, y as usize);
                        final_col = self.blend(final_col, bg);
                    }

                    self.write_vram(x as usize, y as usize, final_col | stp_bit);
                } else if entered {
                    // O triângulo é estritamente convexo: uma vez percorrido na linha, não há mais pixels!
                    break;
                }

                w0 += dw0_dx;
                w1 += dw1_dx;
                w2 += dw2_dx;
                u_sum += d_u_dx;
                v_sum += d_v_dx;
                r_sum += d_r_dx;
                g_sum += d_g_dx;
                b_sum += d_b_dx;
            }

            w0_row += dw0_dy;
            w1_row += dw1_dy;
            w2_row += dw2_dy;
            if do_tex {
                u_row_sum += d_u_dy;
                v_row_sum += d_v_dy;
            }
            if !flat_color {
                r_row_sum += d_r_dy;
                g_row_sum += d_g_dy;
                b_row_sum += d_b_dy;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rasterize_triangle(
        &mut self,
        v0: Vertex,
        v1: Vertex,
        v2: Vertex,
        textured: bool,
        raw_tex: bool,
        semi_transparent: bool,
        apply_dither: bool,
    ) {
        if self.config.render_mode == RenderMode::Hardware {
            self.rasterize_triangle_hardware(v0, v1, v2, textured, raw_tex, semi_transparent, apply_dither);
        } else {
            self.rasterize_triangle_software(v0, v1, v2, textured, raw_tex, semi_transparent, apply_dither);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rasterize_triangle_hardware(
        &mut self,
        v0: Vertex,
        v1: Vertex,
        v2: Vertex,
        textured: bool,
        raw_tex: bool,
        semi_transparent: bool,
        apply_dither: bool,
    ) {
        let min_x = (v0.fx.min(v1.fx).min(v2.fx).floor() as i32)
            .max(self.draw_area_left)
            .max(0);
        let max_x = (v0.fx.max(v1.fx).max(v2.fx).ceil() as i32)
            .min(self.draw_area_right)
            .min((VRAM_WIDTH - 1) as i32);
        let min_y = (v0.fy.min(v1.fy).min(v2.fy).floor() as i32)
            .max(self.draw_area_top)
            .max(0);
        let max_y = (v0.fy.max(v1.fy).max(v2.fy).ceil() as i32)
            .min(self.draw_area_bottom)
            .min((VRAM_HEIGHT - 1) as i32);

        if min_x > max_x || min_y > max_y {
            return;
        }

        let area = (v1.fx - v0.fx) * (v2.fy - v0.fy) - (v1.fy - v0.fy) * (v2.fx - v0.fx);
        if area.abs() < 1e-6 {
            return;
        }
        let (v1, v2, area) = if area < 0.0 {
            (v2, v1, -area)
        } else {
            (v1, v2, area)
        };
        let inv_area = 1.0 / area;

        let flat_color = v0.r == v1.r && v1.r == v2.r && v0.g == v1.g && v1.g == v2.g && v0.b == v1.b && v1.b == v2.b;
        let do_tex = textured && !self.texture_disable;

        let mut clut_cache = [0u16; 256];
        if do_tex {
            match self.texture_depth {
                0 => {
                    for i in 0..16 {
                        clut_cache[i] = self.read_vram(self.clut_x + i, self.clut_y);
                    }
                }
                1 => {
                    for i in 0..256 {
                        clut_cache[i] = self.read_vram(self.clut_x + i, self.clut_y);
                    }
                }
                _ => {}
            }
        }

        let do_persp = self.config.perspective_correct && do_tex;

        let start_px = min_x as f32 + 0.5;
        let start_py = min_y as f32 + 0.5;

        let dw0_dx = -(v2.fy - v1.fy) * inv_area;
        let dw0_dy = (v2.fx - v1.fx) * inv_area;
        let dw1_dx = -(v0.fy - v2.fy) * inv_area;
        let dw1_dy = (v0.fx - v2.fx) * inv_area;
        let dw2_dx = -dw0_dx - dw1_dx;
        let dw2_dy = -dw0_dy - dw1_dy;

        let mut w0_row = ((v2.fx - v1.fx) * (start_py - v1.fy) - (v2.fy - v1.fy) * (start_px - v1.fx)) * inv_area;
        let mut w1_row = ((v0.fx - v2.fx) * (start_py - v2.fy) - (v0.fy - v2.fy) * (start_px - v2.fx)) * inv_area;
        let mut w2_row = 1.0 - w0_row - w1_row;

        for y in min_y..=max_y {
            let dither_row = &DITHER_TABLE[(y & 3) as usize];
            let mut entered = false;
            let mut w0 = w0_row;
            let mut w1 = w1_row;
            let mut w2 = w2_row;

            for x in min_x..=max_x {
                if w0 >= -0.0001 && w1 >= -0.0001 && w2 >= -0.0001 {
                    entered = true;

                    let (r, g, b) = if flat_color {
                        (v0.r as u32, v0.g as u32, v0.b as u32)
                    } else {
                        let cr = (w0 * v0.r as f32 + w1 * v1.r as f32 + w2 * v2.r as f32).clamp(0.0, 255.0) as u32;
                        let cg = (w0 * v0.g as f32 + w1 * v1.g as f32 + w2 * v2.g as f32).clamp(0.0, 255.0) as u32;
                        let cb = (w0 * v0.b as f32 + w1 * v1.b as f32 + w2 * v2.b as f32).clamp(0.0, 255.0) as u32;
                        (cr, cg, cb)
                    };

                    let mut is_semi = semi_transparent;
                    let mut stp_bit = 0u16;

                    let (r5, g5, b5) = if do_tex {
                        let (u_f, v_f) = if do_persp {
                            let w_sum = w0 * v0.w + w1 * v1.w + w2 * v2.w;
                            let inv_w = if w_sum.abs() > 1e-7 { 1.0 / w_sum } else { 1.0 };
                            let u = (w0 * (v0.u as f32 * v0.w) + w1 * (v1.u as f32 * v1.w) + w2 * (v2.u as f32 * v2.w)) * inv_w;
                            let v = (w0 * (v0.v as f32 * v0.w) + w1 * (v1.v as f32 * v1.w) + w2 * (v2.v as f32 * v2.w)) * inv_w;
                            (u, v)
                        } else {
                            let u = w0 * v0.u as f32 + w1 * v1.u as f32 + w2 * v2.u as f32;
                            let v = w0 * v0.v as f32 + w1 * v1.v as f32 + w2 * v2.v as f32;
                            (u, v)
                        };

                        let tex_color = if self.config.bilinear_filter {
                            self.sample_texture_bilinear(u_f, v_f, &clut_cache)
                        } else {
                            self.sample_texture_fast(u_f.clamp(0.0, 255.0) as u16, v_f.clamp(0.0, 255.0) as u16, &clut_cache)
                        };

                        if tex_color == 0 {
                            w0 += dw0_dx;
                            w1 += dw1_dx;
                            w2 += dw2_dx;
                            continue;
                        }

                        stp_bit = tex_color & 0x8000;
                        is_semi = semi_transparent && (stp_bit != 0);

                        let tr = (tex_color & 0x1F) as u32;
                        let tg = ((tex_color >> 5) & 0x1F) as u32;
                        let tb = ((tex_color >> 10) & 0x1F) as u32;

                        if raw_tex {
                            (tr, tg, tb)
                        } else {
                            let r_mod = (tr * r) >> 7;
                            let g_mod = (tg * g) >> 7;
                            let b_mod = (tb * b) >> 7;
                            (r_mod.min(31), g_mod.min(31), b_mod.min(31))
                        }
                    } else {
                        ((r >> 3) & 0x1F, (g >> 3) & 0x1F, (b >> 3) & 0x1F)
                    };

                    let (mut r_out, mut g_out, mut b_out) = (r5 as i16, g5 as i16, b5 as i16);

                    if apply_dither {
                        let d = dither_row[(x & 3) as usize];
                        r_out = ((r_out << 3) + d).clamp(0, 255) >> 3;
                        g_out = ((g_out << 3) + d).clamp(0, 255) >> 3;
                        b_out = ((b_out << 3) + d).clamp(0, 255) >> 3;
                    }

                    let mut final_col = (r_out as u16 & 0x1F)
                        | ((g_out as u16 & 0x1F) << 5)
                        | ((b_out as u16 & 0x1F) << 10);

                    if is_semi {
                        let bg = self.read_vram(x as usize, y as usize);
                        final_col = self.blend(final_col, bg);
                    }

                    self.write_vram(x as usize, y as usize, final_col | stp_bit);
                } else if entered {
                    break;
                }

                w0 += dw0_dx;
                w1 += dw1_dx;
                w2 += dw2_dx;
            }

            w0_row += dw0_dy;
            w1_row += dw1_dy;
            w2_row += dw2_dy;
        }
    }

    fn rasterize_line(&mut self, v0: Vertex, v1: Vertex, semi: bool, shaded: bool) {
        if (v0.x - v1.x).abs() >= 1024 || (v0.y - v1.y).abs() >= 512 {
            return;
        }

        let mut x0 = v0.x;
        let mut y0 = v0.y;
        let x1 = v1.x;
        let y1 = v1.y;

        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        let total_steps = dx.max(-dy).max(1);
        let mut step = 0;

        loop {
            if self.is_inside_scissoring(x0, y0) {
                let mut r = ((v0.r as i32 * (total_steps - step) + v1.r as i32 * step) / total_steps) as i16;
                let mut g = ((v0.g as i32 * (total_steps - step) + v1.g as i32 * step) / total_steps) as i16;
                let mut b = ((v0.b as i32 * (total_steps - step) + v1.b as i32 * step) / total_steps) as i16;

                if shaded && self.dither {
                    let d = DITHER_TABLE[(y0 & 3) as usize][(x0 & 3) as usize];
                    r = (r + d).clamp(0, 255);
                    g = (g + d).clamp(0, 255);
                    b = (b + d).clamp(0, 255);
                }

                let mut col = ((r as u16 >> 3) & 0x1F)
                    | (((g as u16 >> 3) & 0x1F) << 5)
                    | (((b as u16 >> 3) & 0x1F) << 10);

                if semi {
                    let bg = self.read_vram(x0 as usize, y0 as usize);
                    col = self.blend(col, bg);
                }

                self.write_vram(x0 as usize, y0 as usize, col);
            }

            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
            step += 1;
        }
    }

    #[inline(always)]
    fn sample_texture_bilinear(&self, u_f: f32, v_f: f32, clut_cache: &[u16; 256]) -> u16 {
        let u0 = u_f.floor() as i32;
        let v0 = v_f.floor() as i32;
        let frac_x = ((u_f - u0 as f32) * 32.0).clamp(0.0, 31.0) as u32;
        let frac_y = ((v_f - v0 as f32) * 32.0).clamp(0.0, 31.0) as u32;

        if frac_x == 0 && frac_y == 0 {
            return self.sample_texture_fast((u0.max(0) as u16) & 0xFF, (v0.max(0) as u16) & 0xFF, clut_cache);
        }

        let u1 = u0 + 1;
        let v1 = v0 + 1;
        let inv_frac_x = 32 - frac_x;
        let inv_frac_y = 32 - frac_y;

        let c00 = self.sample_texture_fast((u0.max(0) as u16) & 0xFF, (v0.max(0) as u16) & 0xFF, clut_cache);
        let c10 = self.sample_texture_fast((u1.max(0) as u16) & 0xFF, (v0.max(0) as u16) & 0xFF, clut_cache);
        let c01 = self.sample_texture_fast((u0.max(0) as u16) & 0xFF, (v1.max(0) as u16) & 0xFF, clut_cache);
        let c11 = self.sample_texture_fast((u1.max(0) as u16) & 0xFF, (v1.max(0) as u16) & 0xFF, clut_cache);

        if c00 == 0 && c10 == 0 && c01 == 0 && c11 == 0 {
            return 0;
        }

        let c00_val = if c00 != 0 { c00 } else if c10 != 0 { c10 } else { c01 };
        let c10_val = if c10 != 0 { c10 } else { c00_val };
        let c01_val = if c01 != 0 { c01 } else { c00_val };
        let c11_val = if c11 != 0 { c11 } else { c10_val };

        let w00 = inv_frac_x * inv_frac_y;
        let w10 = frac_x * inv_frac_y;
        let w01 = inv_frac_x * frac_y;
        let w11 = frac_x * frac_y;

        let r00 = (c00_val & 0x1F) as u32;
        let g00 = ((c00_val >> 5) & 0x1F) as u32;
        let b00 = ((c00_val >> 10) & 0x1F) as u32;

        let r10 = (c10_val & 0x1F) as u32;
        let g10 = ((c10_val >> 5) & 0x1F) as u32;
        let b10 = ((c10_val >> 10) & 0x1F) as u32;

        let r01 = (c01_val & 0x1F) as u32;
        let g01 = ((c01_val >> 5) & 0x1F) as u32;
        let b01 = ((c01_val >> 10) & 0x1F) as u32;

        let r11 = (c11_val & 0x1F) as u32;
        let g11 = ((c11_val >> 5) & 0x1F) as u32;
        let b11 = ((c11_val >> 10) & 0x1F) as u32;

        let r = (r00 * w00 + r10 * w10 + r01 * w01 + r11 * w11 + 512) >> 10;
        let g = (g00 * w00 + g10 * w10 + g01 * w01 + g11 * w11 + 512) >> 10;
        let b = (b00 * w00 + b10 * w10 + b01 * w01 + b11 * w11 + 512) >> 10;

        let stp = (c00 | c10 | c01 | c11) & 0x8000;
        (r.min(31) as u16) | ((g.min(31) as u16) << 5) | ((b.min(31) as u16) << 10) | stp
    }

    #[inline(always)]
    fn sample_texture_fast(&self, mut u: u16, mut v: u16, clut_cache: &[u16; 256]) -> u16 {
        if self.tex_window_mask_x != 0 || self.tex_window_mask_y != 0 {
            let mask_x = ((self.tex_window_mask_x as u16) * 8) & 0xFF;
            let mask_y = ((self.tex_window_mask_y as u16) * 8) & 0xFF;
            let off_x = ((self.tex_window_offset_x as u16) * 8) & 0xFF;
            let off_y = ((self.tex_window_offset_y as u16) * 8) & 0xFF;

            u = (u & !mask_x) | (off_x & mask_x);
            v = (v & !mask_y) | (off_y & mask_y);
        }

        let u = (u & 0xFF) as usize;
        let v = (v & 0xFF) as usize;

        match self.texture_depth {
            0 => {
                let word = self.read_vram(self.page_base_x + (u / 4), self.page_base_y + v);
                let shift = (u % 4) * 4;
                let clut_idx = ((word >> shift) & 0x0F) as usize;
                clut_cache[clut_idx]
            }
            1 => {
                let word = self.read_vram(self.page_base_x + (u / 2), self.page_base_y + v);
                let shift = (u % 2) * 8;
                let clut_idx = ((word >> shift) & 0xFF) as usize;
                clut_cache[clut_idx]
            }
            _ => self.read_vram(self.page_base_x + u, self.page_base_y + v),
        }
    }

    fn sample_texture(&self, mut u: u16, mut v: u16) -> u16 {
        if self.tex_window_mask_x != 0 || self.tex_window_mask_y != 0 {
            let mask_x = ((self.tex_window_mask_x as u16) * 8) & 0xFF;
            let mask_y = ((self.tex_window_mask_y as u16) * 8) & 0xFF;
            let off_x = ((self.tex_window_offset_x as u16) * 8) & 0xFF;
            let off_y = ((self.tex_window_offset_y as u16) * 8) & 0xFF;

            u = (u & !mask_x) | (off_x & mask_x);
            v = (v & !mask_y) | (off_y & mask_y);
        }

        let u = (u & 0xFF) as usize;
        let v = (v & 0xFF) as usize;

        match self.texture_depth {
            0 => {
                let word = self.read_vram(self.page_base_x + (u / 4), self.page_base_y + v);
                let shift = (u % 4) * 4;
                let clut_idx = ((word >> shift) & 0x0F) as usize;
                self.read_vram(self.clut_x + clut_idx, self.clut_y)
            }
            1 => {
                let word = self.read_vram(self.page_base_x + (u / 2), self.page_base_y + v);
                let shift = (u % 2) * 8;
                let clut_idx = ((word >> shift) & 0xFF) as usize;
                self.read_vram(self.clut_x + clut_idx, self.clut_y)
            }
            _ => self.read_vram(self.page_base_x + u, self.page_base_y + v),
        }
    }

    #[inline(always)]
    fn blend(&self, fg: u16, bg: u16) -> u16 {
        let (fr, fg_g, fb) = (fg & 0x1F, (fg >> 5) & 0x1F, (fg >> 10) & 0x1F);
        let (br, bg_g, bb) = (bg & 0x1F, (bg >> 5) & 0x1F, (bg >> 10) & 0x1F);

        let (r, g, b) = match self.semi_transparency_mode {
            0 => ((br + fr) / 2, (bg_g + fg_g) / 2, (bb + fb) / 2),
            1 => ((br + fr).min(31), (bg_g + fg_g).min(31), (bb + fb).min(31)),
            2 => (
                br.saturating_sub(fr),
                bg_g.saturating_sub(fg_g),
                bb.saturating_sub(fb),
            ),
            _ => (
                (br + (fr / 4)).min(31),
                (bg_g + (fg_g / 4)).min(31),
                (bb + (fb / 4)).min(31),
            ),
        };

        r | (g << 5) | (b << 10)
    }

    pub fn render_to_display_buffer(&self, buffer: &mut [u32]) {
        if self.display_disabled {
            buffer[..TV_WIDTH * TV_HEIGHT].fill(0xFF00_0000);
            return;
        }

        let hres_bits = (self.status >> 17) & 3;
        let hres_2 = (self.status >> 16) & 1;
        let disp_w = if hres_2 != 0 {
            368
        } else {
            match hres_bits {
                0 => 256,
                1 => 320,
                2 => 512,
                _ => 640,
            }
        };
        let disp_h = if (self.status & (1 << 19)) != 0 {
            480
        } else {
            240
        };

        if self.display_depth_24 {
            let mut px_lut = [0usize; TV_WIDTH];
            for x in 0..TV_WIDTH {
                px_lut[x] = (x * disp_w) / TV_WIDTH;
            }

            for y in 0..TV_HEIGHT {
                let src_y = (self.display_vram_y + (y * disp_h) / TV_HEIGHT) & 0x1FF;
                let vram_row = src_y * VRAM_WIDTH;
                let buf_row = y * TV_WIDTH;

                for x in 0..TV_WIDTH {
                    let px = px_lut[x];
                    let hw_idx = self.display_vram_x + (px * 3) / 2;
                    let hw0 = self.vram[vram_row + (hw_idx & 0x3FF)];
                    let hw1 = self.vram[vram_row + ((hw_idx + 1) & 0x3FF)];

                    let (r, g, b) = if (px & 1) == 0 {
                        (
                            (hw0 & 0xFF) as u32,
                            ((hw0 >> 8) & 0xFF) as u32,
                            (hw1 & 0xFF) as u32,
                        )
                    } else {
                        (
                            ((hw0 >> 8) & 0xFF) as u32,
                            (hw1 & 0xFF) as u32,
                            ((hw1 >> 8) & 0xFF) as u32,
                        )
                    };
                    buffer[buf_row + x] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
                }
            }
        } else {
            let mut x_lut = [0usize; TV_WIDTH];
            for x in 0..TV_WIDTH {
                x_lut[x] = (self.display_vram_x + (x * disp_w) / TV_WIDTH) & 0x3FF;
            }

            for y in 0..TV_HEIGHT {
                let src_y = (self.display_vram_y + (y * disp_h) / TV_HEIGHT) & 0x1FF;
                let vram_row = src_y * VRAM_WIDTH;
                let buf_row = y * TV_WIDTH;

                for x in 0..TV_WIDTH {
                    let pixel = self.vram[vram_row + x_lut[x]];
                    let r = ((pixel & 0x1F) as u32) << 3;
                    let g = (((pixel >> 5) & 0x1F) as u32) << 3;
                    let b = (((pixel >> 10) & 0x1F) as u32) << 3;
                    buffer[buf_row + x] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
                }
            }
        }

        if matches!(self.config.shader, ShaderEffect::CrtScanlines | ShaderEffect::BilinearCrt) {
            for y in 0..TV_HEIGHT {
                if (y & 1) != 0 {
                    let row = y * TV_WIDTH;
                    for x in 0..TV_WIDTH {
                        let c = buffer[row + x];
                        let r = (((c >> 16) & 0xFF) * 3) / 4;
                        let g = (((c >> 8) & 0xFF) * 3) / 4;
                        let b = ((c & 0xFF) * 3) / 4;
                        buffer[row + x] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
                    }
                }
            }
        } else if self.config.shader == ShaderEffect::RfNoise {
            static mut NOISE_SEED: u32 = 1234567;
            for y in 0..TV_HEIGHT {
                let row = y * TV_WIDTH;
                for x in 0..TV_WIDTH {
                    let c = buffer[row + x];
                    let r = ((c >> 16) & 0xFF) as i32;
                    let g = ((c >> 8) & 0xFF) as i32;
                    let b = (c & 0xFF) as i32;
                    unsafe {
                        NOISE_SEED = (NOISE_SEED ^ (NOISE_SEED << 13)).wrapping_mul(314159265);
                        let noise = ((NOISE_SEED & 0x0F) as i32) - 8;
                        let scanline = if (y & 1) != 0 { -12 } else { 0 };
                        let nr = (r + noise + scanline).clamp(0, 255) as u32;
                        let ng = (g + noise + scanline).clamp(0, 255) as u32;
                        let nb = (b + noise + scanline).clamp(0, 255) as u32;
                        buffer[row + x] = 0xFF00_0000 | (nr << 16) | (ng << 8) | nb;
                    }
                }
            }
        }
    }

    #[inline(always)]
    fn is_inside_scissoring(&self, x: i32, y: i32) -> bool {
        x >= self.draw_area_left
            && x <= self.draw_area_right
            && y >= self.draw_area_top
            && y <= self.draw_area_bottom
    }

    fn decode_vertex(&self, pos: u32, color: u32, uv: u32) -> Vertex {
        let raw_sx = sign_extend_11(pos & 0x7FF);
        let raw_sy = sign_extend_11((pos >> 16) & 0x7FF);
        let orig_x = raw_sx + self.draw_offset_x;
        let y = raw_sy + self.draw_offset_y;

        let (mut fx, fy, w) = if self.config.render_mode == RenderMode::Hardware && self.config.pgxp_enabled {
            crate::pgxp::lookup_pgxp(raw_sx as i16, raw_sy as i16, self.draw_offset_x, self.draw_offset_y)
        } else {
            (orig_x as f32, y as f32, 1.0)
        };

        // WIDESCREEN HACK 16:9: comprime horizontalmente a geometria 3D por 0.75x (3/4)
        // Quando esticado em tela 16:9 (1.333x), anula a distorção e expande o campo de visão (FOV)!
        let mut x = orig_x;
        if self.config.widescreen_hack {
            let center_x = (self.draw_area_left + self.draw_area_right) as f32 * 0.5;
            let cx = if center_x > 60.0 && center_x < 600.0 { center_x } else { 160.0 };
            fx = cx + (fx - cx) * 0.75;
            x = (cx + (orig_x as f32 - cx) * 0.75).round() as i32;
        }

        Vertex {
            x,
            y,
            fx,
            fy,
            w: if w > 0.000001 { w } else { 1.0 },
            r: color as u8,
            g: (color >> 8) as u8,
            b: (color >> 16) as u8,
            u: (uv & 0xFF) as u16,
            v: ((uv >> 8) & 0xFF) as u16,
        }
    }

    fn decode_clut(&mut self, clut: u32) {
        self.clut_x = ((clut & 0x3F) * 16) as usize;
        self.clut_y = ((clut >> 6) & 0x1FF) as usize;
    }

    fn decode_tpage(&mut self, tpage: u32) {
        self.page_base_x = ((tpage & 0x0F) * 64) as usize;
        self.page_base_y = (((tpage >> 4) & 1) * 256) as usize;
        self.semi_transparency_mode = ((tpage >> 5) & 3) as u8;
        self.texture_depth = ((tpage >> 7) & 3) as u8;
    }

    fn setup_transfer(&mut self, mode: TransferMode) {
        self.trans_x = (self.gp0_buffer[1] & 0x3FF) as usize;
        self.trans_y = ((self.gp0_buffer[1] >> 16) & 0x1FF) as usize;
        self.trans_w = ((self.gp0_buffer[2] & 0xFFFF).wrapping_sub(1) & 0x3FF) as usize + 1;
        self.trans_h =
            (((self.gp0_buffer[2] >> 16) & 0xFFFF).wrapping_sub(1) & 0x1FF) as usize + 1;

        self.trans_cur_x = self.trans_x;
        self.trans_cur_y = self.trans_y;

        let words = (self.trans_w * self.trans_h + 1) / 2;
        if words > 0 {
            self.trans_words_remaining = words as u32;
            self.transfer_mode = mode;
        }
    }

    fn advance_transfer(&mut self) {
        self.trans_cur_x += 1;
        if self.trans_cur_x >= self.trans_x + self.trans_w {
            self.trans_cur_x = self.trans_x;
            self.trans_cur_y += 1;
        }
    }
}

#[inline(always)]
fn edge_function(x0: i32, y0: i32, x1: i32, y1: i32, px: i32, py: i32) -> i32 {
    (px - x0) * (y1 - y0) - (py - y0) * (x1 - x0)
}

#[inline(always)]
fn sign_extend_11(val: u32) -> i32 {
    ((val << 21) as i32) >> 21
}

#[inline(always)]
fn bgr24_to_rgb15(c: u32) -> u16 {
    let r = ((c & 0xFF) >> 3) as u16;
    let g = (((c >> 8) & 0xFF) >> 3) as u16;
    let b = (((c >> 16) & 0xFF) >> 3) as u16;
    r | (g << 5) | (b << 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_quick_fill() {
        let mut gpu = Gpu::new();
        gpu.write_gp0(0x0200_00FF);
        gpu.write_gp0(0x0010_0010);
        gpu.write_gp0(0x0010_0010);
        assert_eq!(gpu.read_vram(16, 16), 0x001F);
        assert_eq!(gpu.read_vram(31, 31), 0x001F);
        assert_eq!(gpu.read_vram(32, 32), 0x0000);
    }

    #[test]
    fn test_cpu_to_vram_image_load() {
        let mut gpu = Gpu::new();
        gpu.write_gp0(0xA000_0000);
        gpu.write_gp0(0x0000_0000);
        gpu.write_gp0(0x0001_0002);
        gpu.write_gp0(0xF678_9234);
        assert_eq!(gpu.read_vram(0, 0), 0x9234);
        assert_eq!(gpu.read_vram(1, 0), 0xF678);
    }

    #[test]
    fn test_monochrome_triangle_rasterization() {
        let mut gpu = Gpu::new();
        gpu.write_gp0(0x20FF_FFFF);
        gpu.write_gp0(0x000A_000A);
        gpu.write_gp0(0x000A_001E);
        gpu.write_gp0(0x001E_0014);
        assert_eq!(gpu.read_vram(20, 15), 0x7FFF);
        assert_eq!(gpu.read_vram(5, 5), 0x0000);
    }

    #[test]
    fn test_24bit_display_mode() {
        let mut gpu = Gpu::new();
        gpu.write_gp1(0x0300_0000); // Display enable
        gpu.write_gp1(0x0800_0030); // 24-bit mode (bit 4=1), 256x240
        assert!(gpu.display_depth_24);
        assert_eq!((gpu.status >> 21) & 1, 1);

        gpu.write_vram(0, 0, 0x00FF);
        gpu.write_vram(1, 0, 0x0000);
        gpu.write_vram(2, 0, 0x00FF);

        let mut buf = [0u32; TV_WIDTH * TV_HEIGHT];
        gpu.render_to_display_buffer(&mut buf);

        assert_eq!(buf[0], 0xFFFF_0000);
        assert_eq!(buf[2], 0xFF00_FF00);
    }
}
