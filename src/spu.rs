// src/spu.rs

use crate::interrupt::{InterruptType, Interrupts};
use std::collections::VecDeque;

pub const SPU_RAM_SIZE_HALF: usize = 256 * 1024; // 512 KB (256k halfwords)

const POS_ADPCM_FILTER: [i32; 5] = [0, 60, 115, 98, 122];
const NEG_ADPCM_FILTER: [i32; 5] = [0, 0, -52, -55, -60];

const GAUSSIAN_TABLE: [i32; 512] = [
    -0x001, -0x001, -0x001, -0x001, -0x001, -0x001, -0x001, -0x001,
    -0x001, -0x001, -0x001, -0x001, -0x001, -0x001, -0x001, -0x001,
     0x000,  0x000,  0x000,  0x000,  0x000,  0x000,  0x000,  0x001,
     0x001,  0x001,  0x001,  0x002,  0x002,  0x002,  0x003,  0x003,
     0x003,  0x004,  0x004,  0x005,  0x005,  0x006,  0x007,  0x007,
     0x008,  0x009,  0x009,  0x00A,  0x00B,  0x00C,  0x00D,  0x00E,
     0x00F,  0x010,  0x011,  0x012,  0x013,  0x015,  0x016,  0x018,
     0x019,  0x01B,  0x01C,  0x01E,  0x020,  0x021,  0x023,  0x025,
     0x027,  0x029,  0x02C,  0x02E,  0x030,  0x033,  0x035,  0x038,
     0x03A,  0x03D,  0x040,  0x043,  0x046,  0x049,  0x04D,  0x050,
     0x054,  0x057,  0x05B,  0x05F,  0x063,  0x067,  0x06B,  0x06F,
     0x074,  0x078,  0x07D,  0x082,  0x087,  0x08C,  0x091,  0x096,
     0x09C,  0x0A1,  0x0A7,  0x0AD,  0x0B3,  0x0BA,  0x0C0,  0x0C7,
     0x0CD,  0x0D4,  0x0DB,  0x0E3,  0x0EA,  0x0F2,  0x0FA,  0x101,
     0x10A,  0x112,  0x11B,  0x123,  0x12C,  0x135,  0x13F,  0x148,
     0x152,  0x15C,  0x166,  0x171,  0x17B,  0x186,  0x191,  0x19C,
     0x1A8,  0x1B4,  0x1C0,  0x1CC,  0x1D9,  0x1E5,  0x1F2,  0x200,
     0x20D,  0x21B,  0x229,  0x237,  0x246,  0x255,  0x264,  0x273,
     0x283,  0x293,  0x2A3,  0x2B4,  0x2C4,  0x2D6,  0x2E7,  0x2F9,
     0x30B,  0x31D,  0x330,  0x343,  0x356,  0x36A,  0x37E,  0x392,
     0x3A7,  0x3BC,  0x3D1,  0x3E7,  0x3FC,  0x413,  0x42A,  0x441,
     0x458,  0x470,  0x488,  0x4A0,  0x4B9,  0x4D2,  0x4EC,  0x506,
     0x520,  0x53B,  0x556,  0x572,  0x58E,  0x5AA,  0x5C7,  0x5E4,
     0x601,  0x61F,  0x63E,  0x65C,  0x67C,  0x69B,  0x6BB,  0x6DC,
     0x6FD,  0x71E,  0x740,  0x762,  0x784,  0x7A7,  0x7CB,  0x7EF,
     0x813,  0x838,  0x85D,  0x883,  0x8A9,  0x8D0,  0x8F7,  0x91E,
     0x946,  0x96F,  0x998,  0x9C1,  0x9EB,  0xA16,  0xA40,  0xA6C,
     0xA98,  0xAC4,  0xAF1,  0xB1E,  0xB4C,  0xB7A,  0xBA9,  0xBD8,
     0xC07,  0xC38,  0xC68,  0xC99,  0xCCB,  0xCFD,  0xD30,  0xD63,
     0xD97,  0xDCB,  0xE00,  0xE35,  0xE6B,  0xEA1,  0xED7,  0xF0F,
     0xF46,  0xF7F,  0xFB7,  0xFF1,  0x102A, 0x1065, 0x109F, 0x10DB,
     0x1116, 0x1153, 0x118F, 0x11CD, 0x120B, 0x1249, 0x1288, 0x12C7,
     0x1307, 0x1347, 0x1388, 0x13C9, 0x140B, 0x144D, 0x1490, 0x14D4,
     0x1517, 0x155C, 0x15A0, 0x15E6, 0x162C, 0x1672, 0x16B9, 0x1700,
     0x1747, 0x1790, 0x17D8, 0x1821, 0x186B, 0x18B5, 0x1900, 0x194B,
     0x1996, 0x19E2, 0x1A2E, 0x1A7B, 0x1AC8, 0x1B16, 0x1B64, 0x1BB3,
     0x1C02, 0x1C51, 0x1CA1, 0x1CF1, 0x1D42, 0x1D93, 0x1DE5, 0x1E37,
     0x1E89, 0x1EDC, 0x1F2F, 0x1F82, 0x1FD6, 0x202A, 0x207F, 0x20D4,
     0x2129, 0x217F, 0x21D5, 0x222C, 0x2282, 0x22DA, 0x2331, 0x2389,
     0x23E1, 0x2439, 0x2492, 0x24EB, 0x2545, 0x259E, 0x25F8, 0x2653,
     0x26AD, 0x2708, 0x2763, 0x27BE, 0x281A, 0x2876, 0x28D2, 0x292E,
     0x298B, 0x29E7, 0x2A44, 0x2AA1, 0x2AFF, 0x2B5C, 0x2BBA, 0x2C18,
     0x2C76, 0x2CD4, 0x2D33, 0x2D91, 0x2DF0, 0x2E4F, 0x2EAE, 0x2F0D,
     0x2F6C, 0x2FCC, 0x302B, 0x308B, 0x30EA, 0x314A, 0x31AA, 0x3209,
     0x3269, 0x32C9, 0x3329, 0x3389, 0x33E9, 0x3449, 0x34A9, 0x3509,
     0x3569, 0x35C9, 0x3629, 0x3689, 0x36E8, 0x3748, 0x37A8, 0x3807,
     0x3867, 0x38C6, 0x3926, 0x3985, 0x39E4, 0x3A43, 0x3AA2, 0x3B00,
     0x3B5F, 0x3BBD, 0x3C1B, 0x3C79, 0x3CD7, 0x3D35, 0x3D92, 0x3DEF,
     0x3E4C, 0x3EA9, 0x3F05, 0x3F62, 0x3FBD, 0x4019, 0x4074, 0x40D0,
     0x412A, 0x4185, 0x41DF, 0x4239, 0x4292, 0x42EB, 0x4344, 0x439C,
     0x43F4, 0x444C, 0x44A3, 0x44FA, 0x4550, 0x45A6, 0x45FC, 0x4651,
     0x46A6, 0x46FA, 0x474E, 0x47A1, 0x47F4, 0x4846, 0x4898, 0x48E9,
     0x493A, 0x498A, 0x49D9, 0x4A29, 0x4A77, 0x4AC5, 0x4B13, 0x4B5F,
     0x4BAC, 0x4BF7, 0x4C42, 0x4C8D, 0x4CD7, 0x4D20, 0x4D68, 0x4DB0,
     0x4DF7, 0x4E3E, 0x4E84, 0x4EC9, 0x4F0E, 0x4F52, 0x4F95, 0x4FD7,
     0x5019, 0x505A, 0x509A, 0x50DA, 0x5118, 0x5156, 0x5194, 0x51D0,
     0x520C, 0x5247, 0x5281, 0x52BA, 0x52F3, 0x532A, 0x5361, 0x5397,
     0x53CC, 0x5401, 0x5434, 0x5467, 0x5499, 0x54CA, 0x54FA, 0x5529,
     0x5558, 0x5585, 0x55B2, 0x55DE, 0x5609, 0x5632, 0x565B, 0x5684,
     0x56AB, 0x56D1, 0x56F6, 0x571B, 0x573E, 0x5761, 0x5782, 0x57A3,
     0x57C3, 0x57E2, 0x57FF, 0x581C, 0x5838, 0x5853, 0x586D, 0x5886,
     0x589E, 0x58B5, 0x58CB, 0x58E0, 0x58F4, 0x5907, 0x5919, 0x592A,
     0x593A, 0x5949, 0x5958, 0x5965, 0x5971, 0x597C, 0x5986, 0x598F,
     0x5997, 0x599E, 0x59A4, 0x59A9, 0x59AD, 0x59B0, 0x59B2, 0x59B3,
];

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum AdsrStage {
    #[default]
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy)]
pub struct Voice {
    pub vol_left: u16,
    pub vol_right: u16,
    pub cur_vol_left: i32,
    pub cur_vol_right: i32,
    pub pitch: u16,
    pub start_addr: u16,
    pub adsr_lo: u16,
    pub adsr_hi: u16,
    pub adsr_vol: i32,
    pub repeat_addr: u16,

    pub cur_addr: u32,
    pub phase_acc: u32,
    pub sample_idx: usize,
    pub stage: AdsrStage,
    pub env_counter: u32,

    pub old_s: i32,
    pub older_s: i32,
    pub decoded_block: [i16; 28],
    pub prev_samples: [i16; 4],
    pub block_decoded: bool,
    pub last_out: i32,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            vol_left: 0,
            vol_right: 0,
            cur_vol_left: 0,
            cur_vol_right: 0,
            pitch: 0,
            start_addr: 0,
            adsr_lo: 0,
            adsr_hi: 0,
            adsr_vol: 0,
            repeat_addr: 0,
            cur_addr: 0,
            phase_acc: 0,
            sample_idx: 0,
            stage: AdsrStage::Off,
            env_counter: 0,
            old_s: 0,
            older_s: 0,
            decoded_block: [0; 28],
            prev_samples: [0; 4],
            block_decoded: false,
            last_out: 0,
        }
    }
}

pub struct Spu {
    pub ram: Box<[u16; SPU_RAM_SIZE_HALF]>,
    pub voices: [Voice; 24],

    pub transfer_addr: u32,
    pub irq_addr: u32,
    pub control: u16,
    pub transfer_control: u16,
    pub status_irq_flag: bool,

    pub main_vol_left: u16,
    pub main_vol_right: u16,
    pub cur_main_left: i32,
    pub cur_main_right: i32,
    pub reverb_vol_left: i16,
    pub reverb_vol_right: i16,
    pub cd_vol_left: i16,
    pub cd_vol_right: i16,

    pub pmon: u32,
    pub non: u32,
    pub eon: u32,
    pub voice_key_on: u32,
    pub endx: u32,

    pub noise_level: i16,
    pub noise_count: u32,

    pub reverb_base: u32,
    pub reverb_cur_addr: u32,
    pub reverb_counter: u8,
    pub reverb_out_l: i32,
    pub reverb_out_r: i32,
    pub prev_reverb_l: i32,
    pub prev_reverb_r: i32,
    pub rev_in_prev_l: i32,
    pub rev_in_prev_r: i32,
    pub capture_index: u32,

    pub regs: [u16; 0x100],
    sample_cycle_acc: u32,

    pub cd_audio_fifo: VecDeque<(i16, i16)>,
    pub audio_out: Vec<i16>,
}

impl Spu {
    pub fn new() -> Self {
        Self {
            ram: vec![0u16; SPU_RAM_SIZE_HALF]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            voices: [Voice::default(); 24],
            transfer_addr: 0,
            irq_addr: 0,
            control: 0,
            transfer_control: 0x0004,
            status_irq_flag: false,
            main_vol_left: 0x3FFF,
            main_vol_right: 0x3FFF,
            cur_main_left: 0x3FFF,
            cur_main_right: 0x3FFF,
            reverb_vol_left: 0,
            reverb_vol_right: 0,
            cd_vol_left: 0x7FFF,
            cd_vol_right: 0x7FFF,
            pmon: 0,
            non: 0,
            eon: 0,
            voice_key_on: 0,
            endx: 0,
            noise_level: 1,
            noise_count: 0,
            reverb_base: 0x3E000,
            reverb_cur_addr: 0,
            reverb_counter: 0,
            reverb_out_l: 0,
            reverb_out_r: 0,
            prev_reverb_l: 0,
            prev_reverb_r: 0,
            rev_in_prev_l: 0,
            rev_in_prev_r: 0,
            capture_index: 0,
            regs: [0; 0x100],
            sample_cycle_acc: 0,
            cd_audio_fifo: VecDeque::with_capacity(16384),
            audio_out: Vec::with_capacity(4096),
        }
    }

    pub fn take_audio_samples(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.audio_out)
    }

    pub fn push_cd_audio(&mut self, left: i16, right: i16) {
        if self.cd_audio_fifo.len() < 32768 {
            self.cd_audio_fifo.push_back((left, right));
        }
    }

    #[inline(always)]
    pub fn read_status(&self) -> u16 {
        let mut stat = self.control & 0x003F;
        if self.status_irq_flag {
            stat |= 1 << 6;
        }
        if ((self.control >> 4) & 3) == 2 {
            stat |= (1 << 7) | (1 << 8);
        } else if ((self.control >> 4) & 3) == 3 {
            stat |= (1 << 7) | (1 << 9);
        }
        if (self.capture_index & 0x100) != 0 {
            stat |= 1 << 11;
        }
        stat
    }

    #[inline(always)]
    fn check_irq_addr(&mut self, addr: u32, irq: &mut Interrupts) {
        if (self.control & 0x8040) == 0x8040 && (addr & 0x3FFF8) == (self.irq_addr & 0x3FFF8) {
            self.status_irq_flag = true;
            irq.trigger(InterruptType::Spu);
        }
    }

    fn decode_adpcm_block(ram: &[u16; SPU_RAM_SIZE_HALF], voice: &mut Voice) {
        let base = (voice.cur_addr as usize) & (SPU_RAM_SIZE_HALF - 8);
        let header = ram[base];

        let mut shift = (header & 0x0F) as u32;
        if shift > 12 {
            shift = 9;
        }
        let filter = (((header >> 4) & 0x07) as usize).min(4);
        let f0 = POS_ADPCM_FILTER[filter];
        let f1 = NEG_ADPCM_FILTER[filter];

        let mut out_idx = 0;
        for hw_idx in 1..8 {
            let mut word = ram[(base + hw_idx) & (SPU_RAM_SIZE_HALF - 1)];
            for _ in 0..4 {
                let nibble = (word & 0x0F) as i16;
                let signed_16 = (nibble << 12) as i32;
                let sample_in = signed_16 >> shift;

                let predicted = sample_in + ((voice.old_s * f0 + voice.older_s * f1 + 32) >> 6);
                let clamped = predicted.clamp(-32768, 32767);

                voice.older_s = voice.old_s;
                voice.old_s = clamped;
                voice.decoded_block[out_idx] = clamped as i16;

                out_idx += 1;
                word >>= 4;
            }
        }
        voice.block_decoded = true;
    }

    #[inline(always)]
    fn gaussian_interpolate(voice: &Voice) -> i32 {
        let g = ((voice.phase_acc >> 4) & 0xFF) as usize;
        let s0 = voice.prev_samples[0] as i32;
        let s1 = voice.prev_samples[1] as i32;
        let s2 = voice.prev_samples[2] as i32;
        let s3 = voice.prev_samples[3] as i32;

        let out = (GAUSSIAN_TABLE[0x0FF - g] * s0
            + GAUSSIAN_TABLE[0x1FF - g] * s1
            + GAUSSIAN_TABLE[0x100 + g] * s2
            + GAUSSIAN_TABLE[g] * s3)
            >> 15;
        out.clamp(-32768, 32767)
    }

    /// Circuito de Envelope ADSR / Sweep unificado de 7 bits exato do hardware da Sony (PSX-SPX)
    #[inline(always)]
    fn advance_envelope(rate: u32, decreasing: bool, exponential: bool, cur_vol: &mut i32, counter: &mut u32) {
        if rate >= 0x7F {
            return;
        }
        let shift = (rate >> 2) & 0x1F;
        let step_idx = (rate & 3) as usize;
        let base_step = if decreasing {
            [-8, -7, -6, -5][step_idx]
        } else {
            [7, 6, 5, 4][step_idx]
        };

        let mut step = if shift < 11 {
            base_step << (11 - shift)
        } else {
            base_step
        };

        let mut period = if shift > 11 {
            1u32 << (shift - 11)
        } else {
            1
        };

        if exponential {
            if decreasing {
                step = ((step as i64 * (*cur_vol as i64)) >> 15).min(-1) as i32;
            } else if *cur_vol > 0x6000 {
                if rate < 40 {
                    step >>= 2;
                } else if rate >= 44 {
                    period <<= 2;
                } else {
                    step >>= 1;
                    period <<= 1;
                }
            }
        }

        *counter += 1;
        if *counter >= period.max(1) {
            *counter = 0;
            *cur_vol = (*cur_vol + step).clamp(0, 0x7FFF);
        }
    }

    #[inline(always)]
    fn step_sweep_volume(reg: u16, cur: &mut i32) -> i32 {
        if (reg & 0x8000) == 0 {
            *cur = (((reg << 1) as i16) >> 1) as i32;
        } else {
            let decreasing = (reg & (1 << 13)) != 0;
            let exponential = (reg & (1 << 14)) != 0;
            let rate = (reg & 0x7F) as u32;
            let mut dummy_cnt = 0;
            Self::advance_envelope(rate, decreasing, exponential, cur, &mut dummy_cnt);
        }
        *cur
    }

    fn step_adsr(voice: &mut Voice) {
        let sustain_level = ((((voice.adsr_lo & 0x0F) as i32) + 1) * 0x800).min(0x7FFF);

        match voice.stage {
            AdsrStage::Off => {
                voice.adsr_vol = 0;
            }
            AdsrStage::Attack => {
                let rate = ((voice.adsr_lo >> 8) & 0x7F) as u32;
                let exp = (voice.adsr_lo & 0x8000) != 0;
                Self::advance_envelope(rate, false, exp, &mut voice.adsr_vol, &mut voice.env_counter);
                if voice.adsr_vol >= 0x7FFF {
                    voice.adsr_vol = 0x7FFF;
                    voice.env_counter = 0;
                    voice.stage = AdsrStage::Decay;
                }
            }
            AdsrStage::Decay => {
                let rate = (((voice.adsr_lo >> 4) & 0x0F) << 2) as u32;
                Self::advance_envelope(rate, true, true, &mut voice.adsr_vol, &mut voice.env_counter);
                if voice.adsr_vol <= sustain_level {
                    voice.adsr_vol = sustain_level;
                    voice.env_counter = 0;
                    voice.stage = AdsrStage::Sustain;
                }
            }
            AdsrStage::Sustain => {
                let rate = ((voice.adsr_hi >> 6) & 0x7F) as u32;
                let dec = (voice.adsr_hi & (1 << 14)) != 0;
                let exp = (voice.adsr_hi & (1 << 15)) != 0;
                Self::advance_envelope(rate, dec, exp, &mut voice.adsr_vol, &mut voice.env_counter);
            }
            AdsrStage::Release => {
                let rate = ((voice.adsr_hi & 0x1F) << 2) as u32;
                let exp = (voice.adsr_hi & (1 << 5)) != 0;
                Self::advance_envelope(rate, true, exp, &mut voice.adsr_vol, &mut voice.env_counter);
                if voice.adsr_vol == 0 {
                    voice.stage = AdsrStage::Off;
                }
            }
        }
    }

    fn step_noise(&mut self) {
        let shift = ((self.control >> 10) & 0x0F) as u32;
        let step = (4 + ((self.control >> 8) & 3) as u32).max(4);
        self.noise_count += step;
        let threshold = 0x8000u32 >> shift.min(15);
        if self.noise_count >= threshold.max(1) {
            self.noise_count = 0;
            let bit = ((self.noise_level >> 15)
                ^ (self.noise_level >> 12)
                ^ (self.noise_level >> 11)
                ^ (self.noise_level >> 10)
                ^ 1)
                & 1;
            self.noise_level = (self.noise_level << 1) | bit;
        }
    }

    /// Calcula endereço na área de Reverb da SPU RAM com offset preciso em halfwords
    #[inline(always)]
    fn rev_addr_hw(&self, hw_offset: isize) -> usize {
        let base = (self.reverb_base as usize).max(0x800).min(SPU_RAM_SIZE_HALF - 2);
        let size = (SPU_RAM_SIZE_HALF - base) as isize;
        let rel = ((self.reverb_cur_addr as isize) + hw_offset).rem_euclid(size) as usize;
        (base + rel) & (SPU_RAM_SIZE_HALF - 1)
    }

    #[inline(always)]
    fn rev_read_hw(&self, hw_offset: isize) -> i32 {
        (self.ram[self.rev_addr_hw(hw_offset)] as i16) as i32
    }

    #[inline(always)]
    fn rev_write_hw(&mut self, hw_offset: isize, val: i32) {
        if (self.control & (1 << 7)) != 0 {
            let addr = self.rev_addr_hw(hw_offset);
            self.ram[addr] = (val.clamp(-32768, 32767) as i16) as u16;
        }
    }

    /// Executa o Hardware Reverb da Sony com endereçamento exato em halfwords (mLSAME*4 - 1) e saturação de 16 bits
    fn process_hardware_reverb(&mut self, in_l: i32, in_r: i32) {
        // Filtro passa-baixa anti-aliasing de entrada (44.1 kHz -> 22.05 kHz)
        let filt_in_l = (in_l + self.rev_in_prev_l) >> 1;
        let filt_in_r = (in_r + self.rev_in_prev_r) >> 1;
        self.rev_in_prev_l = in_l;
        self.rev_in_prev_r = in_r;

        self.reverb_counter ^= 1;
        if self.reverb_counter == 0 {
            return;
        }

        let sat16 = |v: i32| -> i32 { v.clamp(-32768, 32767) };
        let hw = |reg: usize| -> isize { (self.regs[reg] as isize) << 2 };

        let d_apf1 = hw(0xE0);
        let d_apf2 = hw(0xE1);
        let v_iir = (self.regs[0xE2] as i16) as i32;
        let v_comb1 = (self.regs[0xE3] as i16) as i32;
        let v_comb2 = (self.regs[0xE4] as i16) as i32;
        let v_comb3 = (self.regs[0xE5] as i16) as i32;
        let v_comb4 = (self.regs[0xE6] as i16) as i32;
        let v_wall = (self.regs[0xE7] as i16) as i32;
        let v_apf1 = (self.regs[0xE8] as i16) as i32;
        let v_apf2 = (self.regs[0xE9] as i16) as i32;

        let m_lsame = hw(0xEA);
        let m_rsame = hw(0xEB);
        let m_lcomb1 = hw(0xEC);
        let m_rcomb1 = hw(0xED);
        let m_lcomb2 = hw(0xEE);
        let m_rcomb2 = hw(0xEF);
        let d_lsame = hw(0xF0);
        let d_rsame = hw(0xF1);
        let m_ldiff = hw(0xF2);
        let m_rdiff = hw(0xF3);
        let m_lcomb3 = hw(0xF4);
        let m_rcomb3 = hw(0xF5);
        let m_lcomb4 = hw(0xF6);
        let m_rcomb4 = hw(0xF7);
        let d_ldiff = hw(0xF8);
        let d_rdiff = hw(0xF9);
        let m_lapf1 = hw(0xFA);
        let m_rapf1 = hw(0xFB);
        let m_lapf2 = hw(0xFC);
        let m_rapf2 = hw(0xFD);

        let v_lin = (self.regs[0xFE] as i16) as i32;
        let v_rin = (self.regs[0xFF] as i16) as i32;

        let lin = (v_lin * filt_in_l) >> 15;
        let rin = (v_rin * filt_in_r) >> 15;

        // Reflexões Same-Side (com atraso exato de 1 halfword: m_lsame - 1)
        let l_prev = self.rev_read_hw(m_lsame - 1);
        let l_same = sat16(sat16(lin + ((self.rev_read_hw(d_lsame) * v_wall) >> 15)) - l_prev);
        self.rev_write_hw(m_lsame, sat16(l_prev + ((l_same * v_iir) >> 15)));

        let r_prev = self.rev_read_hw(m_rsame - 1);
        let r_same = sat16(sat16(rin + ((self.rev_read_hw(d_rsame) * v_wall) >> 15)) - r_prev);
        self.rev_write_hw(m_rsame, sat16(r_prev + ((r_same * v_iir) >> 15)));

        // Reflexões Diff-Side (com atraso exato de 1 halfword: m_ldiff - 1)
        let ld_prev = self.rev_read_hw(m_ldiff - 1);
        let l_diff = sat16(sat16(lin + ((self.rev_read_hw(d_rdiff) * v_wall) >> 15)) - ld_prev);
        self.rev_write_hw(m_ldiff, sat16(ld_prev + ((l_diff * v_iir) >> 15)));

        let rd_prev = self.rev_read_hw(m_rdiff - 1);
        let r_diff = sat16(sat16(rin + ((self.rev_read_hw(d_ldiff) * v_wall) >> 15)) - rd_prev);
        self.rev_write_hw(m_rdiff, sat16(rd_prev + ((r_diff * v_iir) >> 15)));

        // Filtros Comb de 4 estágios com saturação intermediária
        let mut l_out = sat16(
            ((v_comb1 * self.rev_read_hw(m_lcomb1)) >> 15)
                + ((v_comb2 * self.rev_read_hw(m_lcomb2)) >> 15)
                + ((v_comb3 * self.rev_read_hw(m_lcomb3)) >> 15)
                + ((v_comb4 * self.rev_read_hw(m_lcomb4)) >> 15),
        );
        let mut r_out = sat16(
            ((v_comb1 * self.rev_read_hw(m_rcomb1)) >> 15)
                + ((v_comb2 * self.rev_read_hw(m_rcomb2)) >> 15)
                + ((v_comb3 * self.rev_read_hw(m_rcomb3)) >> 15)
                + ((v_comb4 * self.rev_read_hw(m_rcomb4)) >> 15),
        );

        // All-Pass Filter 1
        let lapf1_del = self.rev_read_hw(m_lapf1 - d_apf1);
        l_out = sat16(l_out - ((v_apf1 * lapf1_del) >> 15));
        self.rev_write_hw(m_lapf1, l_out);
        l_out = sat16(((l_out * v_apf1) >> 15) + lapf1_del);

        let rapf1_del = self.rev_read_hw(m_rapf1 - d_apf1);
        r_out = sat16(r_out - ((v_apf1 * rapf1_del) >> 15));
        self.rev_write_hw(m_rapf1, r_out);
        r_out = sat16(((r_out * v_apf1) >> 15) + rapf1_del);

        // All-Pass Filter 2
        let lapf2_del = self.rev_read_hw(m_lapf2 - d_apf2);
        l_out = sat16(l_out - ((v_apf2 * lapf2_del) >> 15));
        self.rev_write_hw(m_lapf2, l_out);
        l_out = sat16(((l_out * v_apf2) >> 15) + lapf2_del);

        let rapf2_del = self.rev_read_hw(m_rapf2 - d_apf2);
        r_out = sat16(r_out - ((v_apf2 * rapf2_del) >> 15));
        self.rev_write_hw(m_rapf2, r_out);
        r_out = sat16(((r_out * v_apf2) >> 15) + rapf2_del);

        self.reverb_out_l = (l_out * (self.reverb_vol_left as i32)) >> 15;
        self.reverb_out_r = (r_out * (self.reverb_vol_right as i32)) >> 15;

        let base = self.reverb_base.max(0x800);
        let size = (SPU_RAM_SIZE_HALF as u32 - base).max(1);
        self.reverb_cur_addr = (self.reverb_cur_addr + 1) % size;
    }

    pub fn tick(&mut self, cycles: u32, irq: &mut Interrupts) {
        self.sample_cycle_acc += cycles;
        if self.sample_cycle_acc < 768 {
            return;
        }
        let samples = self.sample_cycle_acc / 768;
        self.sample_cycle_acc %= 768;

        let spu_enabled = (self.control & (1 << 15)) != 0;
        let unmuted = (self.control & (1 << 14)) != 0;

        for _ in 0..samples {
            self.step_noise();

            let mut mix_left = 0i32;
            let mut mix_right = 0i32;
            let mut rev_in_l = 0i32;
            let mut rev_in_r = 0i32;

            if spu_enabled {
                for i in 0..24 {
                    if self.voices[i].stage == AdsrStage::Off {
                        self.voices[i].last_out = 0;
                        continue;
                    }

                    let cur_addr = self.voices[i].cur_addr;
                    self.check_irq_addr(cur_addr, irq);

                    if !self.voices[i].block_decoded {
                        Self::decode_adpcm_block(&self.ram, &mut self.voices[i]);
                        self.voices[i].prev_samples[2] = self.voices[i].decoded_block[0];
                        self.voices[i].prev_samples[3] = self.voices[i].decoded_block[1];
                        self.voices[i].sample_idx = 1;
                    }

                    Self::step_adsr(&mut self.voices[i]);
                    if self.voices[i].stage == AdsrStage::Off {
                        self.voices[i].last_out = 0;
                        continue;
                    }

                    let raw_sample = if (self.non & (1 << i)) != 0 {
                        self.noise_level as i32
                    } else {
                        Self::gaussian_interpolate(&self.voices[i])
                    };

                    let env_sample = (raw_sample * self.voices[i].adsr_vol) >> 15;
                    self.voices[i].last_out = env_sample;

                    let mut v_l = self.voices[i].cur_vol_left;
                    let mut v_r = self.voices[i].cur_vol_right;
                    v_l = Self::step_sweep_volume(self.voices[i].vol_left, &mut v_l);
                    v_r = Self::step_sweep_volume(self.voices[i].vol_right, &mut v_r);
                    self.voices[i].cur_vol_left = v_l;
                    self.voices[i].cur_vol_right = v_r;

                    let out_l = (env_sample * v_l) >> 14;
                    let out_r = (env_sample * v_r) >> 14;

                    mix_left += out_l;
                    mix_right += out_r;

                    if (self.eon & (1 << i)) != 0 {
                        rev_in_l += out_l;
                        rev_in_r += out_r;
                    }

                    let mut step = self.voices[i].pitch as i32;
                    if i > 0 && (self.pmon & (1 << i)) != 0 {
                        let factor = self.voices[i - 1].last_out + 0x8000;
                        let signed_pitch = (self.voices[i].pitch as i16) as i32;
                        step = (signed_pitch * factor) >> 15;
                    }
                    let pitch_step = (step.clamp(0, 0x3FFF)) as u32;

                    self.voices[i].phase_acc += pitch_step;
                    while self.voices[i].phase_acc >= 0x1000 {
                        self.voices[i].phase_acc -= 0x1000;
                        self.voices[i].sample_idx += 1;

                        if self.voices[i].sample_idx >= 28 {
                            self.voices[i].sample_idx = 0;

                            let block_addr =
                                (self.voices[i].cur_addr as usize) & (SPU_RAM_SIZE_HALF - 8);
                            let header = self.ram[block_addr];
                            let flags = (header >> 8) & 0x07;

                            if (flags & 0x04) != 0 {
                                self.voices[i].repeat_addr = (block_addr >> 2) as u16;
                            }

                            self.voices[i].cur_addr =
                                ((block_addr + 8) & (SPU_RAM_SIZE_HALF - 1)) as u32;

                            if (flags & 0x01) != 0 {
                                self.endx |= 1 << i;
                                if (flags & 0x02) != 0 {
                                    self.voices[i].cur_addr =
                                        ((self.voices[i].repeat_addr as u32) << 2) & 0x3FFFF;
                                } else {
                                    self.voices[i].adsr_vol = 0;
                                    self.voices[i].stage = AdsrStage::Off;
                                    break;
                                }
                            }
                            Self::decode_adpcm_block(&self.ram, &mut self.voices[i]);
                        }

                        self.voices[i].prev_samples[0] = self.voices[i].prev_samples[1];
                        self.voices[i].prev_samples[1] = self.voices[i].prev_samples[2];
                        self.voices[i].prev_samples[2] = self.voices[i].prev_samples[3];
                        self.voices[i].prev_samples[3] =
                            self.voices[i].decoded_block[self.voices[i].sample_idx];
                    }
                }
            }

            let (cd_l, cd_r) = self.cd_audio_fifo.pop_front().unwrap_or((0, 0));
            if (self.control & 0x01) != 0 {
                let cl = ((cd_l as i32) * (self.cd_vol_left as i32)) >> 15;
                let cr = ((cd_r as i32) * (self.cd_vol_right as i32)) >> 15;
                mix_left += cl;
                mix_right += cr;
                if (self.control & 0x04) != 0 {
                    rev_in_l += cl;
                    rev_in_r += cr;
                }
            }

            let cap = (self.capture_index & 0x1FF) as usize;
            self.ram[cap] = cd_l as u16;
            self.ram[0x200 + cap] = cd_r as u16;
            self.ram[0x400 + cap] = (self.voices[1].last_out.clamp(-32768, 32767) as i16) as u16;
            self.ram[0x600 + cap] = (self.voices[3].last_out.clamp(-32768, 32767) as i16) as u16;
            self.check_irq_addr(cap as u32, irq);
            self.check_irq_addr((0x200 + cap) as u32, irq);
            self.check_irq_addr((0x400 + cap) as u32, irq);
            self.check_irq_addr((0x600 + cap) as u32, irq);
            self.capture_index = (self.capture_index + 1) & 0x1FF;

            let reverb_active = (self.control & (1 << 7)) != 0
                || (self.reverb_vol_left != 0 || self.reverb_vol_right != 0);

            if reverb_active {
                let prev_l = self.reverb_out_l;
                let prev_r = self.reverb_out_r;
                self.process_hardware_reverb(
                    rev_in_l.clamp(-32768, 32767),
                    rev_in_r.clamp(-32768, 32767),
                );
                if self.reverb_counter == 0 {
                    mix_left += (self.prev_reverb_l + self.reverb_out_l) >> 1;
                    mix_right += (self.prev_reverb_r + self.reverb_out_r) >> 1;
                } else {
                    self.prev_reverb_l = prev_l;
                    self.prev_reverb_r = prev_r;
                    mix_left += self.reverb_out_l;
                    mix_right += self.reverb_out_r;
                }
            }

            if unmuted {
                let mut ml = self.cur_main_left;
                let mut mr = self.cur_main_right;
                ml = Self::step_sweep_volume(self.main_vol_left, &mut ml);
                mr = Self::step_sweep_volume(self.main_vol_right, &mut mr);
                self.cur_main_left = ml;
                self.cur_main_right = mr;

                mix_left = ((mix_left * ml) >> 14).clamp(-32768, 32767);
                mix_right = ((mix_right * mr) >> 14).clamp(-32768, 32767);
            } else {
                mix_left = 0;
                mix_right = 0;
            }

            self.audio_out.push(mix_left as i16);
            self.audio_out.push(mix_right as i16);
        }
    }

    pub fn read(&mut self, offset: u32) -> u16 {
        let index = (offset >> 1) as usize;
        if index >= 0x100 {
            if (0x0200..0x0260).contains(&offset) {
                let v = ((offset - 0x0200) >> 2) as usize;
                if v < 24 {
                    if (offset & 2) == 0 {
                        return self.voices[v].cur_vol_left as u16;
                    } else {
                        return self.voices[v].cur_vol_right as u16;
                    }
                }
            }
            return 0;
        }

        if index < 0xC0 {
            let v = index >> 3;
            let reg = index & 7;
            return match reg {
                0 => self.voices[v].vol_left,
                1 => self.voices[v].vol_right,
                2 => self.voices[v].pitch,
                3 => self.voices[v].start_addr,
                4 => self.voices[v].adsr_lo,
                5 => self.voices[v].adsr_hi,
                6 => self.voices[v].adsr_vol as u16,
                7 => self.voices[v].repeat_addr,
                _ => 0,
            };
        }

        match offset {
            0x0180 => self.main_vol_left,
            0x0182 => self.main_vol_right,
            0x0184 => self.reverb_vol_left as u16,
            0x0186 => self.reverb_vol_right as u16,
            0x0190 => (self.pmon & 0xFFFF) as u16,
            0x0192 => (self.pmon >> 16) as u16,
            0x0194 => (self.non & 0xFFFF) as u16,
            0x0196 => (self.non >> 16) as u16,
            0x0198 => (self.eon & 0xFFFF) as u16,
            0x019A => (self.eon >> 16) as u16,
            0x019C => (self.endx & 0xFFFF) as u16,
            0x019E => ((self.endx >> 16) & 0xFF) as u16,
            0x01A2 => (self.reverb_base >> 2) as u16,
            0x01A4 => (self.irq_addr >> 2) as u16,
            0x01A6 => (self.transfer_addr >> 2) as u16,
            0x01A8 => self.read_fifo_internal(),
            0x01AA => self.control,
            0x01AC => self.transfer_control,
            0x01AE => self.read_status(),
            0x01B0 => self.cd_vol_left as u16,
            0x01B2 => self.cd_vol_right as u16,
            0x01B8 => self.cur_main_left as u16,
            0x01BA => self.cur_main_right as u16,
            _ => self.regs[index],
        }
    }

    pub fn write(&mut self, offset: u32, val: u16) {
        let index = (offset >> 1) as usize;
        if index < 0x100 {
            self.regs[index] = val;
        }

        if index < 0xC0 {
            let v = index >> 3;
            let reg = index & 7;
            match reg {
                0 => {
                    self.voices[v].vol_left = val;
                    if (val & 0x8000) == 0 {
                        self.voices[v].cur_vol_left = (((val << 1) as i16) >> 1) as i32;
                    }
                }
                1 => {
                    self.voices[v].vol_right = val;
                    if (val & 0x8000) == 0 {
                        self.voices[v].cur_vol_right = (((val << 1) as i16) >> 1) as i32;
                    }
                }
                2 => self.voices[v].pitch = val,
                3 => self.voices[v].start_addr = val,
                4 => self.voices[v].adsr_lo = val,
                5 => self.voices[v].adsr_hi = val,
                6 => self.voices[v].adsr_vol = (val & 0x7FFF) as i32,
                7 => self.voices[v].repeat_addr = val,
                _ => {}
            }
            return;
        }

        match offset {
            0x0180 => {
                self.main_vol_left = val;
                if (val & 0x8000) == 0 {
                    self.cur_main_left = (((val << 1) as i16) >> 1) as i32;
                }
            }
            0x0182 => {
                self.main_vol_right = val;
                if (val & 0x8000) == 0 {
                    self.cur_main_right = (((val << 1) as i16) >> 1) as i32;
                }
            }
            0x0184 => self.reverb_vol_left = val as i16,
            0x0186 => self.reverb_vol_right = val as i16,
            0x0188 => {
                self.voice_key_on = (self.voice_key_on & !0xFFFF) | (val as u32);
                for i in 0..16 {
                    if (val & (1 << i)) != 0 {
                        self.key_on_voice(i);
                    }
                }
            }
            0x018A => {
                let hi = (val & 0xFF) as u32;
                self.voice_key_on = (self.voice_key_on & 0xFFFF) | (hi << 16);
                for i in 0..8 {
                    if (hi & (1 << i)) != 0 {
                        self.key_on_voice(16 + i);
                    }
                }
            }
            0x018C => {
                for i in 0..16 {
                    if (val & (1 << i)) != 0 {
                        self.voices[i].stage = AdsrStage::Release;
                        self.voices[i].env_counter = 0;
                    }
                }
            }
            0x018E => {
                for i in 0..8 {
                    if (val & (1 << i)) != 0 {
                        self.voices[16 + i].stage = AdsrStage::Release;
                        self.voices[16 + i].env_counter = 0;
                    }
                }
            }
            0x0190 => self.pmon = (self.pmon & !0xFFFF) | ((val & !1) as u32),
            0x0192 => self.pmon = (self.pmon & 0xFFFF) | (((val & 0xFF) as u32) << 16),
            0x0194 => self.non = (self.non & !0xFFFF) | (val as u32),
            0x0196 => self.non = (self.non & 0xFFFF) | (((val & 0xFF) as u32) << 16),
            0x0198 => self.eon = (self.eon & !0xFFFF) | (val as u32),
            0x019A => self.eon = (self.eon & 0xFFFF) | (((val & 0xFF) as u32) << 16),
            0x01A2 => {
                self.reverb_base = ((val as u32) << 2) & 0x0003_FFFF;
                self.reverb_cur_addr = 0;
            }
            0x01A4 => {
                self.irq_addr = ((val as u32) << 2) & 0x0003_FFFF;
            }
            0x01A6 => {
                self.transfer_addr = ((val as u32) << 2) & 0x0003_FFFF;
            }
            0x01A8 => {
                self.write_fifo_internal(val);
            }
            0x01AA => {
                self.control = val;
                if (val & 0x0040) == 0 {
                    self.status_irq_flag = false;
                }
            }
            0x01AC => {
                self.transfer_control = val;
            }
            0x01B0 => self.cd_vol_left = val as i16,
            0x01B2 => self.cd_vol_right = val as i16,
            _ => {}
        }
    }

    fn key_on_voice(&mut self, v: usize) {
        let start = ((self.voices[v].start_addr as u32) << 2) & 0x0003_FFFF;
        self.voices[v].cur_addr = start;
        self.voices[v].repeat_addr = self.voices[v].start_addr;
        self.voices[v].phase_acc = 0;
        self.voices[v].sample_idx = 0;
        self.voices[v].adsr_vol = 0;
        self.voices[v].env_counter = 0;
        self.voices[v].stage = AdsrStage::Attack;
        self.voices[v].old_s = 0;
        self.voices[v].older_s = 0;
        self.voices[v].prev_samples = [0; 4];
        self.voices[v].block_decoded = false;
        self.endx &= !(1 << v);
    }

    #[inline(always)]
    pub fn write_fifo(&mut self, val: u16, irq: &mut Interrupts) {
        let addr = (self.transfer_addr as usize) & (SPU_RAM_SIZE_HALF - 1);
        self.ram[addr] = val;
        self.check_irq_addr(self.transfer_addr, irq);
        self.transfer_addr = self.transfer_addr.wrapping_add(1) & 0x0003_FFFF;
    }

    #[inline(always)]
    pub fn read_fifo(&mut self, irq: &mut Interrupts) -> u16 {
        let addr = (self.transfer_addr as usize) & (SPU_RAM_SIZE_HALF - 1);
        let val = self.ram[addr];
        self.check_irq_addr(self.transfer_addr, irq);
        self.transfer_addr = self.transfer_addr.wrapping_add(1) & 0x0003_FFFF;
        val
    }

    #[inline(always)]
    pub fn write_fifo_internal(&mut self, val: u16) {
        let addr = (self.transfer_addr as usize) & (SPU_RAM_SIZE_HALF - 1);
        self.ram[addr] = val;
        if (self.control & 0x8040) == 0x8040
            && (self.transfer_addr & 0x3FFF8) == (self.irq_addr & 0x3FFF8)
        {
            self.status_irq_flag = true;
        }
        self.transfer_addr = self.transfer_addr.wrapping_add(1) & 0x0003_FFFF;
    }

    #[inline(always)]
    pub fn read_fifo_internal(&mut self) -> u16 {
        let addr = (self.transfer_addr as usize) & (SPU_RAM_SIZE_HALF - 1);
        let val = self.ram[addr];
        if (self.control & 0x8040) == 0x8040
            && (self.transfer_addr & 0x3FFF8) == (self.irq_addr & 0x3FFF8)
        {
            self.status_irq_flag = true;
        }
        self.transfer_addr = self.transfer_addr.wrapping_add(1) & 0x0003_FFFF;
        val
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spu_control_and_status() {
        let mut spu = Spu::new();
        spu.write(0x01AA, (1 << 15) | 0x002B);
        assert_eq!(spu.read(0x01AA), (1 << 15) | 0x002B);

        let stat = spu.read_status();
        assert_eq!(stat & 0x003F, 0x002B);
        assert_eq!(stat & (1 << 10), 0);
    }

    #[test]
    fn test_spu_ram_transfer_fifo() {
        let mut spu = Spu::new();
        spu.write(0x01A6, 0x0100);
        spu.write(0x01A8, 0x1234);
        spu.write(0x01A8, 0x5678);
        assert_eq!(spu.ram[0x0400], 0x1234);
        assert_eq!(spu.ram[0x0401], 0x5678);
    }

    #[test]
    fn test_voice_key_on_key_off() {
        let mut spu = Spu::new();
        spu.endx = 0xFFFF_FFFF;

        spu.write(0x0188, 1 << 0);
        spu.write(0x018A, 1 << 1);
        assert_eq!(spu.voices[0].stage, AdsrStage::Attack);
        assert_eq!(spu.voices[17].stage, AdsrStage::Attack);
        assert_eq!(spu.endx & (1 << 0), 0);
        assert_eq!(spu.endx & (1 << 17), 0);

        spu.write(0x018C, 1 << 0);
        assert_eq!(spu.voices[0].stage, AdsrStage::Release);
        assert_eq!(spu.voices[17].stage, AdsrStage::Attack);
    }
}
