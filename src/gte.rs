// src/gte.rs

pub struct Gte {
    pub v: [[i16; 3]; 4],
    pub rgb: (u8, u8, u8, u8),
    pub otz: u16,
    pub ir: [i16; 4],
    pub sxy: [[i16; 2]; 4],
    pub sz: [u16; 4],
    pub rgb_fifo: [(u8, u8, u8, u8); 3],
    pub reg_23: u32,
    pub mac: [i32; 4],
    pub lzcs: u32,
    pub lzcr: u8,

    pub r: [[i16; 3]; 3],
    pub tr: [i32; 3],
    pub l: [[i16; 3]; 3],
    pub bk: [i32; 3],
    pub c: [[i16; 3]; 3],
    pub fc: [i32; 3],
    pub ofx: i32,
    pub ofy: i32,
    pub h: u16,
    pub dqa: i16,
    pub dqb: i32,
    pub zsf3: i16,
    pub zsf4: i16,
    pub flags: u32,
}

impl Gte {
    pub fn new() -> Self {
        Self {
            v: [[0; 3]; 4],
            rgb: (0, 0, 0, 0),
            otz: 0,
            ir: [0; 4],
            sxy: [[0; 2]; 4],
            sz: [0; 4],
            rgb_fifo: [(0, 0, 0, 0); 3],
            reg_23: 0,
            mac: [0; 4],
            lzcs: 0,
            lzcr: 32,
            r: [[0; 3]; 3],
            tr: [0; 3],
            l: [[0; 3]; 3],
            bk: [0; 3],
            c: [[0; 3]; 3],
            fc: [0; 3],
            ofx: 0,
            ofy: 0,
            h: 0,
            dqa: 0,
            dqb: 0,
            zsf3: 0,
            zsf4: 0,
            flags: 0,
        }
    }

    pub fn read_data(&self, reg: usize) -> u32 {
        match reg {
            0 => (self.v[0][0] as u16 as u32) | ((self.v[0][1] as u16 as u32) << 16),
            1 => self.v[0][2] as i32 as u32,
            2 => (self.v[1][0] as u16 as u32) | ((self.v[1][1] as u16 as u32) << 16),
            3 => self.v[1][2] as i32 as u32,
            4 => (self.v[2][0] as u16 as u32) | ((self.v[2][1] as u16 as u32) << 16),
            5 => self.v[2][2] as i32 as u32,
            6 => {
                let (r, g, b, c) = self.rgb;
                (r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | ((c as u32) << 24)
            }
            7 => self.otz as u32,
            8..=11 => self.ir[reg - 8] as i32 as u32,
            12 => (self.sxy[0][0] as u16 as u32) | ((self.sxy[0][1] as u16 as u32) << 16),
            13 => (self.sxy[1][0] as u16 as u32) | ((self.sxy[1][1] as u16 as u32) << 16),
            14 | 15 => (self.sxy[2][0] as u16 as u32) | ((self.sxy[2][1] as u16 as u32) << 16),
            16..=19 => self.sz[reg - 16] as u32,
            20..=22 => {
                let (r, g, b, c) = self.rgb_fifo[reg - 20];
                (r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | ((c as u32) << 24)
            }
            23 => self.reg_23,
            24..=27 => self.mac[reg - 24] as u32,
            28 | 29 => {
                let c5 = |val: i16| -> u32 { ((val >> 7).clamp(0, 0x1F)) as u32 };
                c5(self.ir[1]) | (c5(self.ir[2]) << 5) | (c5(self.ir[3]) << 10)
            }
            30 => self.lzcs,
            31 => self.lzcr as u32,
            _ => 0,
        }
    }

    pub fn write_data(&mut self, reg: usize, val: u32) {
        match reg {
            0 => {
                self.v[0][0] = val as i16;
                self.v[0][1] = (val >> 16) as i16;
            }
            1 => self.v[0][2] = val as i16,
            2 => {
                self.v[1][0] = val as i16;
                self.v[1][1] = (val >> 16) as i16;
            }
            3 => self.v[1][2] = val as i16,
            4 => {
                self.v[2][0] = val as i16;
                self.v[2][1] = (val >> 16) as i16;
            }
            5 => self.v[2][2] = val as i16,
            6 => {
                self.rgb = (
                    val as u8,
                    (val >> 8) as u8,
                    (val >> 16) as u8,
                    (val >> 24) as u8,
                );
            }
            7 => self.otz = val as u16,
            8..=11 => self.ir[reg - 8] = val as i16,
            12 => self.sxy[0] = [val as i16, (val >> 16) as i16],
            13 => self.sxy[1] = [val as i16, (val >> 16) as i16],
            14 => {
                let xy = [val as i16, (val >> 16) as i16];
                self.sxy[2] = xy;
                self.sxy[3] = xy;
            }
            15 => {
                let xy = [val as i16, (val >> 16) as i16];
                self.sxy[3] = xy;
                self.sxy[0] = self.sxy[1];
                self.sxy[1] = self.sxy[2];
                self.sxy[2] = self.sxy[3];
            }
            16..=19 => self.sz[reg - 16] = val as u16,
            20..=22 => {
                self.rgb_fifo[reg - 20] = (
                    val as u8,
                    (val >> 8) as u8,
                    (val >> 16) as u8,
                    (val >> 24) as u8,
                );
            }
            23 => self.reg_23 = val,
            24..=27 => self.mac[reg - 24] = val as i32,
            28 => {
                self.ir[1] = ((val & 0x1F) << 7) as i16;
                self.ir[2] = (((val >> 5) & 0x1F) << 7) as i16;
                self.ir[3] = (((val >> 10) & 0x1F) << 7) as i16;
            }
            29 => {}
            30 => {
                self.lzcs = val;
                let tmp = if (val >> 31) != 0 {
                    !val
                } else {
                    val
                };
                self.lzcr = if val == 0x8000_0000 {
                    0
                } else {
                    tmp.leading_zeros() as u8
                };
            }
            _ => {}
        }
    }

    pub fn read_ctrl(&self, reg: usize) -> u32 {
        match reg {
            0 => (self.r[0][0] as u16 as u32) | ((self.r[0][1] as u16 as u32) << 16),
            1 => (self.r[0][2] as u16 as u32) | ((self.r[1][0] as u16 as u32) << 16),
            2 => (self.r[1][1] as u16 as u32) | ((self.r[1][2] as u16 as u32) << 16),
            3 => (self.r[2][0] as u16 as u32) | ((self.r[2][1] as u16 as u32) << 16),
            4 => self.r[2][2] as i32 as u32,
            5..=7 => self.tr[reg - 5] as u32,
            8 => (self.l[0][0] as u16 as u32) | ((self.l[0][1] as u16 as u32) << 16),
            9 => (self.l[0][2] as u16 as u32) | ((self.l[1][0] as u16 as u32) << 16),
            10 => (self.l[1][1] as u16 as u32) | ((self.l[1][2] as u16 as u32) << 16),
            11 => (self.l[2][0] as u16 as u32) | ((self.l[2][1] as u16 as u32) << 16),
            12 => self.l[2][2] as i32 as u32,
            13..=15 => self.bk[reg - 13] as u32,
            16 => (self.c[0][0] as u16 as u32) | ((self.c[0][1] as u16 as u32) << 16),
            17 => (self.c[0][2] as u16 as u32) | ((self.c[1][0] as u16 as u32) << 16),
            18 => (self.c[1][1] as u16 as u32) | ((self.c[1][2] as u16 as u32) << 16),
            19 => (self.c[2][0] as u16 as u32) | ((self.c[2][1] as u16 as u32) << 16),
            20 => self.c[2][2] as i32 as u32,
            21..=23 => self.fc[reg - 21] as u32,
            24 => self.ofx as u32,
            25 => self.ofy as u32,
            26 => self.h as i16 as u32,
            27 => self.dqa as i32 as u32,
            28 => self.dqb as u32,
            29 => self.zsf3 as i32 as u32,
            30 => self.zsf4 as i32 as u32,
            31 => self.flags,
            _ => 0,
        }
    }

    pub fn write_ctrl(&mut self, reg: usize, val: u32) {
        match reg {
            0 => {
                self.r[0][0] = val as i16;
                self.r[0][1] = (val >> 16) as i16;
            }
            1 => {
                self.r[0][2] = val as i16;
                self.r[1][0] = (val >> 16) as i16;
            }
            2 => {
                self.r[1][1] = val as i16;
                self.r[1][2] = (val >> 16) as i16;
            }
            3 => {
                self.r[2][0] = val as i16;
                self.r[2][1] = (val >> 16) as i16;
            }
            4 => self.r[2][2] = val as i16,
            5..=7 => self.tr[reg - 5] = val as i32,
            8 => {
                self.l[0][0] = val as i16;
                self.l[0][1] = (val >> 16) as i16;
            }
            9 => {
                self.l[0][2] = val as i16;
                self.l[1][0] = (val >> 16) as i16;
            }
            10 => {
                self.l[1][1] = val as i16;
                self.l[1][2] = (val >> 16) as i16;
            }
            11 => {
                self.l[2][0] = val as i16;
                self.l[2][1] = (val >> 16) as i16;
            }
            12 => self.l[2][2] = val as i16,
            13..=15 => self.bk[reg - 13] = val as i32,
            16 => {
                self.c[0][0] = val as i16;
                self.c[0][1] = (val >> 16) as i16;
            }
            17 => {
                self.c[0][2] = val as i16;
                self.c[1][0] = (val >> 16) as i16;
            }
            18 => {
                self.c[1][1] = val as i16;
                self.c[1][2] = (val >> 16) as i16;
            }
            19 => {
                self.c[2][0] = val as i16;
                self.c[2][1] = (val >> 16) as i16;
            }
            20 => self.c[2][2] = val as i16,
            21..=23 => self.fc[reg - 21] = val as i32,
            24 => self.ofx = val as i32,
            25 => self.ofy = val as i32,
            26 => self.h = val as u16,
            27 => self.dqa = val as i16,
            28 => self.dqb = val as i32,
            29 => self.zsf3 = val as i16,
            30 => self.zsf4 = val as i16,
            31 => {
                self.flags = val & 0x7FFF_F000;
                if (val & 0x7F87_E000) != 0 {
                    self.flags |= 1 << 31;
                }
            }
            _ => {}
        }
    }

    pub fn execute(&mut self, cmd: u32) {
        let opcode = cmd & 0x3F;
        let shift = if (cmd & (1 << 19)) != 0 { 12 } else { 0 };
        let clamp_neg = (cmd & (1 << 10)) != 0;
        self.flags = 0;

        match opcode {
            0x01 => {
                let pf = self.transform_vertex(0, shift, clamp_neg);
                self.depth_queuing(pf);
            }
            0x06 => self.cmd_nclip(),
            0x0C => self.cmd_op(shift, clamp_neg),
            0x10 => self.cmd_dpcs(shift, clamp_neg, false),
            0x11 => self.cmd_intpl(shift, clamp_neg),
            0x12 => self.cmd_mvmva(cmd, shift, clamp_neg),
            0x13 => self.do_ncd(0, shift, clamp_neg),
            0x14 => self.cmd_cdp(shift, clamp_neg),
            0x16 => {
                self.do_ncd(0, shift, clamp_neg);
                self.do_ncd(1, shift, clamp_neg);
                self.do_ncd(2, shift, clamp_neg);
            }
            0x1B => self.do_ncc(0, shift, clamp_neg),
            0x1C => self.cmd_cc(shift, clamp_neg),
            0x1E => self.do_nc(0, shift, clamp_neg),
            0x20 => {
                self.do_nc(0, shift, clamp_neg);
                self.do_nc(1, shift, clamp_neg);
                self.do_nc(2, shift, clamp_neg);
            }
            0x28 => self.cmd_sqr(shift, clamp_neg),
            0x29 => self.cmd_dcpl(shift, clamp_neg),
            0x2A => {
                self.cmd_dpcs(shift, clamp_neg, true);
                self.cmd_dpcs(shift, clamp_neg, true);
                self.cmd_dpcs(shift, clamp_neg, true);
            }
            0x2D => self.cmd_avsz3(),
            0x2E => self.cmd_avsz4(),
            0x30 => {
                self.transform_vertex(0, shift, clamp_neg);
                self.transform_vertex(1, shift, clamp_neg);
                let pf = self.transform_vertex(2, shift, clamp_neg);
                self.depth_queuing(pf);
            }
            0x3D => self.cmd_gpf(shift, clamp_neg),
            0x3E => self.cmd_gpl(shift, clamp_neg),
            0x3F => {
                self.do_ncc(0, shift, clamp_neg);
                self.do_ncc(1, shift, clamp_neg);
                self.do_ncc(2, shift, clamp_neg);
            }
            _ => {}
        }

        if (self.flags & 0x7F87_E000) != 0 {
            self.flags |= 1 << 31;
        }
    }

    fn transform_vertex(&mut self, v_idx: usize, shift: u32, clamp_neg: bool) -> u32 {
        let vx = self.v[v_idx][0] as i64;
        let vy = self.v[v_idx][1] as i64;
        let vz = self.v[v_idx][2] as i64;

        // Soma em 44 bits com sinal (igual ao hardware real)
        let sign_ext44 = |val: i64| -> i64 { (val << 20) >> 20 };

        let mut x = sign_ext44((self.tr[0] as i64) << 12);
        x = sign_ext44(x + vx * (self.r[0][0] as i64));
        x = sign_ext44(x + vy * (self.r[0][1] as i64));
        x = sign_ext44(x + vz * (self.r[0][2] as i64));

        let mut y = sign_ext44((self.tr[1] as i64) << 12);
        y = sign_ext44(y + vx * (self.r[1][0] as i64));
        y = sign_ext44(y + vy * (self.r[1][1] as i64));
        y = sign_ext44(y + vz * (self.r[1][2] as i64));

        let mut z = sign_ext44((self.tr[2] as i64) << 12);
        z = sign_ext44(z + vx * (self.r[2][0] as i64));
        z = sign_ext44(z + vy * (self.r[2][1] as i64));
        z = sign_ext44(z + vz * (self.r[2][2] as i64));

        self.mac[1] = (x >> shift) as i32;
        self.mac[2] = (y >> shift) as i32;
        self.mac[3] = (z >> shift) as i32;

        let min_ir = if clamp_neg { 0 } else { -32768 };
        self.ir[1] = self.mac[1].clamp(min_ir, 32767) as i16;
        self.ir[2] = self.mac[2].clamp(min_ir, 32767) as i16;

        // Comportamento peculiar do GTE no PS1: IR3 usa z >> 12 para checagem de limite
        let z_shifted = (z >> 12) as i32;
        if z_shifted < -32768 || z_shifted > 32767 {
            self.flags |= 1 << 22;
        }
        self.ir[3] = self.mac[3].clamp(min_ir, 32767) as i16;

        let sz_val = z_shifted.clamp(0, 0xFFFF) as u16;
        self.sz[0] = self.sz[1];
        self.sz[1] = self.sz[2];
        self.sz[2] = self.sz[3];
        self.sz[3] = sz_val;

        let projection = if sz_val > (self.h / 2) {
            divide(self.h, sz_val)
        } else {
            self.flags |= 1 << 17;
            0x1FFFF
        };

        let raw_x = self.ofx as i64 + (self.ir[1] as i64 * projection as i64);
        let raw_y = self.ofy as i64 + (self.ir[2] as i64 * projection as i64);
        let sx = (raw_x >> 16).clamp(-1024, 1023) as i16;
        let sy = (raw_y >> 16).clamp(-1024, 1023) as i16;

        let prec_x = (raw_x as f32) / 65536.0;
        let prec_y = (raw_y as f32) / 65536.0;
        let prec_w = 1.0 / (sz_val.max(1) as f32);
        crate::pgxp::record_pgxp(sx, sy, prec_x, prec_y, prec_w);

        self.sxy[0] = self.sxy[1];
        self.sxy[1] = self.sxy[2];
        self.sxy[2] = [sx, sy];
        self.sxy[3] = [sx, sy];

        projection
    }

    fn depth_queuing(&mut self, pf: u32) {
        let depth = (self.dqb as i64) + (self.dqa as i64) * (pf as i64);
        self.mac[0] = depth as i32;
        self.ir[0] = (depth >> 12).clamp(0, 4096) as i16;
    }

    fn mac_to_ir(&mut self, clamp_neg: bool) {
        let min_ir = if clamp_neg { 0 } else { -32768 };
        self.ir[1] = self.mac[1].clamp(min_ir, 32767) as i16;
        self.ir[2] = self.mac[2].clamp(min_ir, 32767) as i16;
        self.ir[3] = self.mac[3].clamp(min_ir, 32767) as i16;
    }

    fn mac_to_rgb_fifo(&mut self) {
        let r = (self.mac[1] >> 4).clamp(0, 0xFF) as u8;
        let g = (self.mac[2] >> 4).clamp(0, 0xFF) as u8;
        let b = (self.mac[3] >> 4).clamp(0, 0xFF) as u8;
        let (_, _, _, c) = self.rgb;

        self.rgb_fifo[0] = self.rgb_fifo[1];
        self.rgb_fifo[1] = self.rgb_fifo[2];
        self.rgb_fifo[2] = (r, g, b, c);
    }

    fn multiply_matrix_vec(&mut self, mat: [[i16; 3]; 3], vec: [i16; 3], add: [i32; 3], shift: u32, clamp_neg: bool) {
        let sign_ext44 = |val: i64| -> i64 { (val << 20) >> 20 };

        for r in 0..3 {
            let mut res = sign_ext44((add[r] as i64) << 12);
            for c in 0..3 {
                let prod = (mat[r][c] as i64) * (vec[c] as i64);
                res = sign_ext44(res + prod);
            }
            self.mac[r + 1] = (res >> shift) as i32;
        }
        self.mac_to_ir(clamp_neg);
    }

    fn cmd_mvmva(&mut self, cmd: u32, shift: u32, clamp_neg: bool) {
        self.v[3] = [self.ir[1], self.ir[2], self.ir[3]];
        let mat = match (cmd >> 17) & 3 {
            0 => self.r,
            1 => self.l,
            2 => self.c,
            _ => [[0; 3]; 3],
        };
        let vec_idx = ((cmd >> 15) & 3) as usize;
        let add = match (cmd >> 13) & 3 {
            0 => self.tr,
            1 => self.bk,
            2 => self.fc,
            _ => [0; 3],
        };
        self.multiply_matrix_vec(mat, self.v[vec_idx], add, shift, clamp_neg);
    }

    fn do_nc(&mut self, v_idx: usize, shift: u32, clamp_neg: bool) {
        let l = self.l;
        let v = self.v[v_idx];
        self.multiply_matrix_vec(l, v, [0; 3], shift, clamp_neg);
        let ir_vec = [self.ir[1], self.ir[2], self.ir[3]];
        let c = self.c;
        let bk = self.bk;
        self.multiply_matrix_vec(c, ir_vec, bk, shift, clamp_neg);
        self.mac_to_rgb_fifo();
    }

    fn do_ncc(&mut self, v_idx: usize, shift: u32, clamp_neg: bool) {
        let l = self.l;
        let v = self.v[v_idx];
        self.multiply_matrix_vec(l, v, [0; 3], shift, clamp_neg);
        let ir_vec = [self.ir[1], self.ir[2], self.ir[3]];
        let c = self.c;
        let bk = self.bk;
        self.multiply_matrix_vec(c, ir_vec, bk, shift, clamp_neg);

        let col = [self.rgb.0, self.rgb.1, self.rgb.2];
        for i in 0..3 {
            self.mac[i + 1] = (((col[i] as i32) << 4) * (self.ir[i + 1] as i32)) >> shift;
        }
        self.mac_to_ir(clamp_neg);
        self.mac_to_rgb_fifo();
    }

    fn do_ncd(&mut self, v_idx: usize, shift: u32, clamp_neg: bool) {
        let l = self.l;
        let v = self.v[v_idx];
        self.multiply_matrix_vec(l, v, [0; 3], shift, clamp_neg);
        let ir_vec = [self.ir[1], self.ir[2], self.ir[3]];
        let c = self.c;
        let bk = self.bk;
        self.multiply_matrix_vec(c, ir_vec, bk, shift, clamp_neg);
        self.cmd_dcpl(shift, clamp_neg);
    }

    fn cmd_cc(&mut self, shift: u32, clamp_neg: bool) {
        let ir_vec = [self.ir[1], self.ir[2], self.ir[3]];
        let c = self.c;
        let bk = self.bk;
        self.multiply_matrix_vec(c, ir_vec, bk, shift, clamp_neg);
        let col = [self.rgb.0, self.rgb.1, self.rgb.2];
        for i in 0..3 {
            self.mac[i + 1] = (((col[i] as i32) << 4) * (self.ir[i + 1] as i32)) >> shift;
        }
        self.mac_to_ir(clamp_neg);
        self.mac_to_rgb_fifo();
    }

    fn cmd_cdp(&mut self, shift: u32, clamp_neg: bool) {
        let ir_vec = [self.ir[1], self.ir[2], self.ir[3]];
        let c = self.c;
        let bk = self.bk;
        self.multiply_matrix_vec(c, ir_vec, bk, shift, clamp_neg);
        self.cmd_dcpl(shift, clamp_neg);
    }

    fn cmd_dcpl(&mut self, shift: u32, clamp_neg: bool) {
        let col = [self.rgb.0, self.rgb.1, self.rgb.2];
        let ir0 = self.ir[0] as i64;
        for i in 0..3 {
            let fc = (self.fc[i] as i64) << 12;
            let shading = ((col[i] as i32) << 4) as i64 * (self.ir[i + 1] as i64);
            let tmp = ((fc - shading) >> shift).clamp(-32768, 32767);
            self.mac[i + 1] = ((shading + ir0 * tmp) >> shift) as i32;
        }
        self.mac_to_ir(clamp_neg);
        self.mac_to_rgb_fifo();
    }

    fn cmd_dpcs(&mut self, shift: u32, clamp_neg: bool, from_fifo: bool) {
        let (r, g, b, _) = if from_fifo { self.rgb_fifo[0] } else { self.rgb };
        let col = [r, g, b];
        let ir0 = self.ir[0] as i64;
        for i in 0..3 {
            let fc = (self.fc[i] as i64) << 12;
            let c_val = (col[i] as i64) << 16;
            let tmp = ((fc - c_val) >> shift).clamp(-32768, 32767);
            self.mac[i + 1] = ((c_val + ir0 * tmp) >> shift) as i32;
        }
        self.mac_to_ir(clamp_neg);
        self.mac_to_rgb_fifo();
    }

    fn cmd_intpl(&mut self, shift: u32, clamp_neg: bool) {
        let ir0 = self.ir[0] as i64;
        for i in 0..3 {
            let fc = (self.fc[i] as i64) << 12;
            let ir = (self.ir[i + 1] as i64) << 12;
            let tmp = ((fc - ir) >> shift).clamp(-32768, 32767);
            self.mac[i + 1] = ((ir + ir0 * tmp) >> shift) as i32;
        }
        self.mac_to_ir(clamp_neg);
        self.mac_to_rgb_fifo();
    }

    fn cmd_op(&mut self, shift: u32, clamp_neg: bool) {
        let ir1 = self.ir[1] as i32;
        let ir2 = self.ir[2] as i32;
        let ir3 = self.ir[3] as i32;
        let r0 = self.r[0][0] as i32;
        let r1 = self.r[1][1] as i32;
        let r2 = self.r[2][2] as i32;

        self.mac[1] = (r1 * ir3 - r2 * ir2) >> shift;
        self.mac[2] = (r2 * ir1 - r0 * ir3) >> shift;
        self.mac[3] = (r0 * ir2 - r1 * ir1) >> shift;
        self.mac_to_ir(clamp_neg);
    }

    fn cmd_sqr(&mut self, shift: u32, clamp_neg: bool) {
        for i in 1..4 {
            let ir = self.ir[i] as i32;
            self.mac[i] = (ir * ir) >> shift;
        }
        self.mac_to_ir(clamp_neg);
    }

    fn cmd_gpf(&mut self, shift: u32, clamp_neg: bool) {
        let ir0 = self.ir[0] as i32;
        for i in 1..4 {
            self.mac[i] = ((self.ir[i] as i32) * ir0) >> shift;
        }
        self.mac_to_ir(clamp_neg);
        self.mac_to_rgb_fifo();
    }

    fn cmd_gpl(&mut self, shift: u32, clamp_neg: bool) {
        let ir0 = self.ir[0] as i64;
        for i in 1..4 {
            let prod = (self.ir[i] as i64) * ir0;
            let base = (self.mac[i] as i64) << shift;
            self.mac[i] = ((base + prod) >> shift) as i32;
        }
        self.mac_to_ir(clamp_neg);
        self.mac_to_rgb_fifo();
    }

    fn cmd_nclip(&mut self) {
        let (x0, y0) = (self.sxy[0][0] as i32, self.sxy[0][1] as i32);
        let (x1, y1) = (self.sxy[1][0] as i32, self.sxy[1][1] as i32);
        let (x2, y2) = (self.sxy[2][0] as i32, self.sxy[2][1] as i32);
        self.mac[0] = x0 * (y1 - y2) + x1 * (y2 - y0) + x2 * (y0 - y1);
    }

    fn cmd_avsz3(&mut self) {
        let sum = self.sz[1] as u32 + self.sz[2] as u32 + self.sz[3] as u32;
        let avg = (self.zsf3 as i64 * sum as i64) as i32;
        self.mac[0] = avg;
        self.otz = (avg >> 12).clamp(0, 0xFFFF) as u16;
    }

    fn cmd_avsz4(&mut self) {
        let sum = self.sz[0] as u32 + self.sz[1] as u32 + self.sz[2] as u32 + self.sz[3] as u32;
        let avg = (self.zsf4 as i64 * sum as i64) as i32;
        self.mac[0] = avg;
        self.otz = (avg >> 12).clamp(0, 0xFFFF) as u16;
    }
}

fn divide(numerator: u16, divisor: u16) -> u32 {
    let shift = divisor.leading_zeros();
    let n = (numerator as u64) << shift;
    let d = (divisor as u64) << shift;

    let reciprocal = reciprocal(d as u16) as u64;
    let res = (n * reciprocal + 0x8000) >> 16;

    if res <= 0x1FFFF {
        res as u32
    } else {
        0x1FFFF
    }
}

fn reciprocal(d: u16) -> u32 {
    let index = (((d & 0x7FFF) + 0x40) >> 7) as usize;
    let factor = UNR_TABLE[index] as i32 + 0x101;
    let d = (d | 0x8000) as i32;
    let tmp = ((d * -factor) + 0x80) >> 8;
    let r = ((factor * (0x20000 + tmp)) + 0x80) >> 8;
    r as u32
}

const UNR_TABLE: [u8; 0x101] = [
    0xff, 0xfd, 0xfb, 0xf9, 0xf7, 0xf5, 0xf3, 0xf1, 0xef, 0xee, 0xec, 0xea, 0xe8, 0xe6, 0xe4, 0xe3,
    0xe1, 0xdf, 0xdd, 0xdc, 0xda, 0xd8, 0xd6, 0xd5, 0xd3, 0xd1, 0xd0, 0xce, 0xcd, 0xcb, 0xc9, 0xc8,
    0xc6, 0xc5, 0xc3, 0xc1, 0xc0, 0xbe, 0xbd, 0xbb, 0xba, 0xb8, 0xb7, 0xb5, 0xb4, 0xb2, 0xb1, 0xb0,
    0xae, 0xad, 0xab, 0xaa, 0xa9, 0xa7, 0xa6, 0xa4, 0xa3, 0xa2, 0xa0, 0x9f, 0x9e, 0x9c, 0x9b, 0x9a,
    0x99, 0x97, 0x96, 0x95, 0x94, 0x92, 0x91, 0x90, 0x8f, 0x8d, 0x8c, 0x8b, 0x8a, 0x89, 0x87, 0x86,
    0x85, 0x84, 0x83, 0x82, 0x81, 0x7f, 0x7e, 0x7d, 0x7c, 0x7b, 0x7a, 0x79, 0x78, 0x77, 0x75, 0x74,
    0x73, 0x72, 0x71, 0x70, 0x6f, 0x6e, 0x6d, 0x6c, 0x6b, 0x6a, 0x69, 0x68, 0x67, 0x66, 0x65, 0x64,
    0x63, 0x62, 0x61, 0x60, 0x5f, 0x5e, 0x5d, 0x5d, 0x5c, 0x5b, 0x5a, 0x59, 0x58, 0x57, 0x56, 0x55,
    0x54, 0x53, 0x53, 0x52, 0x51, 0x50, 0x4f, 0x4e, 0x4d, 0x4d, 0x4c, 0x4b, 0x4a, 0x49, 0x48, 0x48,
    0x47, 0x46, 0x45, 0x44, 0x43, 0x43, 0x42, 0x41, 0x40, 0x3f, 0x3f, 0x3e, 0x3d, 0x3c, 0x3c, 0x3b,
    0x3a, 0x39, 0x39, 0x38, 0x37, 0x36, 0x36, 0x35, 0x34, 0x33, 0x33, 0x32, 0x31, 0x31, 0x30, 0x2f,
    0x2e, 0x2e, 0x2d, 0x2c, 0x2c, 0x2b, 0x2a, 0x2a, 0x29, 0x28, 0x28, 0x27, 0x26, 0x26, 0x25, 0x24,
    0x24, 0x23, 0x22, 0x22, 0x21, 0x20, 0x20, 0x1f, 0x1e, 0x1e, 0x1d, 0x1d, 0x1c, 0x1b, 0x1b, 0x1a,
    0x19, 0x19, 0x18, 0x18, 0x17, 0x16, 0x16, 0x15, 0x15, 0x14, 0x14, 0x13, 0x12, 0x12, 0x11, 0x11,
    0x10, 0x0f, 0x0f, 0x0e, 0x0e, 0x0d, 0x0d, 0x0c, 0x0c, 0x0b, 0x0a, 0x0a, 0x09, 0x09, 0x08, 0x08,
    0x07, 0x07, 0x06, 0x06, 0x05, 0x05, 0x04, 0x04, 0x03, 0x03, 0x02, 0x02, 0x01, 0x01, 0x00, 0x00,
    0x00,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gte_lzcr() {
        let mut gte = Gte::new();
        gte.write_data(30, 0x0000_0001);
        assert_eq!(gte.read_data(31), 31);

        gte.write_data(30, 0x8000_0000);
        assert_eq!(gte.read_data(31), 0);
    }

    #[test]
    fn test_gte_hardware_divider() {
        assert_eq!(divide(0, 1234), 0);
        assert_eq!(divide(1, 1), 0x10000);
        assert_eq!(divide(1, 2), 0x8000);
        assert_eq!(divide(10, 40), 0x4000);
    }

    #[test]
    fn test_gte_nclip() {
        let mut gte = Gte::new();
        gte.sxy[0] = [10, 10];
        gte.sxy[1] = [30, 10];
        gte.sxy[2] = [20, 30];
        gte.execute(0x06);
        assert!(gte.mac[0] > 0);
    }

    #[test]
    fn test_gte_avsz3() {
        let mut gte = Gte::new();
        gte.sz[1] = 100;
        gte.sz[2] = 200;
        gte.sz[3] = 300;
        gte.zsf3 = 1365;
        gte.execute(0x2D);
        assert_eq!(gte.otz, 199);
    }
}
