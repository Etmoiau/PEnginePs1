// src/cdrom.rs

use crate::disc::Disc;
use crate::interrupt::{InterruptType, Interrupts};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubCpuState {
    Idle,
    AwaitingResponse2(u8, u32),
    AwaitingGetId(u32),
    Reading(u32),
    PlayingCdDa(u32),
}

pub struct CdRom {
    pub index: u8,
    pub status: u8,
    pub param_fifo: Vec<u8>,
    pub response_fifo: Vec<u8>,
    pub irq_mask: u8,
    pub irq_flags: u8,
    pub drive_mode: u8,
    pub filter_file: u8,
    pub filter_channel: u8,
    pub pending_command: Option<(u8, u32)>,
    pub sub_cpu: SubCpuState,
    pub seek_lba: u32,
    pub sector_buffer: Box<[u8; 2352]>,
    pub latch_buffer: Box<[u8; 2352]>,
    pub sector_read_index: usize,
    pub sector_start_offset: usize,
    pub data_ready: bool,
    pub rx_active: bool,
    pub has_disc: bool,
    pub disc: Option<Disc>,
    pub mixer_vol: [u8; 4],
    pub xa_hist_l: (i32, i32),
    pub xa_hist_r: (i32, i32),
    pub audio_samples: Vec<(i16, i16)>,
    pub xa_pcm_l: Vec<i16>,
    pub xa_pcm_r: Vec<i16>,
}

impl CdRom {
    pub fn new() -> Self {
        Self {
            index: 0,
            status: 0b0001_1000,
            param_fifo: Vec::with_capacity(16),
            response_fifo: Vec::with_capacity(16),
            irq_mask: 0,
            irq_flags: 0,
            drive_mode: 0,
            filter_file: 0,
            filter_channel: 0,
            pending_command: None,
            sub_cpu: SubCpuState::Idle,
            seek_lba: 0,
            sector_buffer: vec![0u8; 2352].into_boxed_slice().try_into().unwrap(),
            latch_buffer: vec![0u8; 2352].into_boxed_slice().try_into().unwrap(),
            sector_read_index: 24,
            sector_start_offset: 24,
            data_ready: false,
            rx_active: false,
            has_disc: true,
            disc: None,
            mixer_vol: [0x80, 0x00, 0x80, 0x00],
            xa_hist_l: (0, 0),
            xa_hist_r: (0, 0),
            audio_samples: Vec::with_capacity(4096),
            xa_pcm_l: Vec::with_capacity(4032),
            xa_pcm_r: Vec::with_capacity(4032),
        }
    }

    #[inline(always)]
    fn update_irq_line(&self, irq: &mut Interrupts) {
        if (self.irq_flags & self.irq_mask & 0x1F) != 0 {
            irq.trigger(InterruptType::CdRom);
        }
    }

    fn set_irq(&mut self, code: u8, irq: &mut Interrupts) {
        self.irq_flags = (self.irq_flags & !0x07) | (code & 0x07);
        self.update_irq_line(irq);
    }

    pub fn tick(&mut self, cycles: u32, irq: &mut Interrupts) {
        if let Some((cmd, rem)) = self.pending_command {
            let next_rem = rem.saturating_sub(cycles);
            if next_rem == 0 && self.irq_flags == 0 {
                self.pending_command = None;
                self.execute_command(cmd, irq);
            } else {
                self.pending_command = Some((cmd, next_rem));
            }
        }

        match self.sub_cpu {
            SubCpuState::AwaitingResponse2(stat, remaining) => {
                let next_rem = remaining.saturating_sub(cycles);
                if next_rem == 0 && self.irq_flags == 0 && self.pending_command.is_none() {
                    self.sub_cpu = SubCpuState::Idle;
                    self.response_fifo.clear();
                    self.response_fifo.push(stat);
                    self.set_irq(2, irq);
                } else {
                    self.sub_cpu = SubCpuState::AwaitingResponse2(stat, next_rem);
                }
            }
            SubCpuState::AwaitingGetId(remaining) => {
                let next_rem = remaining.saturating_sub(cycles);
                if next_rem == 0 && self.irq_flags == 0 && self.pending_command.is_none() {
                    self.sub_cpu = SubCpuState::Idle;
                    self.response_fifo.clear();
                    if self.has_disc {
                        self.response_fifo.extend_from_slice(&[
                            0x02, 0x00, 0x20, 0x00, b'S', b'C', b'E', b'A',
                        ]);
                        self.set_irq(2, irq);
                    } else {
                        self.response_fifo.extend_from_slice(&[
                            0x08, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                        ]);
                        self.set_irq(5, irq);
                    }
                } else {
                    self.sub_cpu = SubCpuState::AwaitingGetId(next_rem);
                }
            }
            SubCpuState::Reading(remaining) => {
                let next_rem = remaining.saturating_sub(cycles);
                if next_rem == 0 && self.irq_flags == 0 && self.pending_command.is_none() {
                    if let Some(disc) = &mut self.disc {
                        let _ = disc.read_sector_raw(self.seek_lba, &mut self.latch_buffer);
                    }

                    let abs_frames = self.seek_lba + 150;
                    let m = (abs_frames / (75 * 60)) as u8;
                    let s = ((abs_frames / 75) % 60) as u8;
                    let f = (abs_frames % 75) as u8;
                    let to_bcd = |v: u8| ((v / 10) << 4) | (v % 10);
                    self.latch_buffer[12] = to_bcd(m);
                    self.latch_buffer[13] = to_bcd(s);
                    self.latch_buffer[14] = to_bcd(f);
                    self.latch_buffer[15] = 0x02;

                    self.seek_lba = self.seek_lba.wrapping_add(1);

                    let speed_cycles = if (self.drive_mode & 0x80) != 0 {
                        225_000
                    } else {
                        450_000
                    };
                    self.sub_cpu = SubCpuState::Reading(speed_cycles);

                    // Verifica se o setor atual é um setor de Áudio XA-ADPCM em tempo real:
                    // Subheader XA: byte 16 = File, byte 17 = Channel, byte 18 = Submode
                    let xa_enabled = (self.drive_mode & 0x40) != 0;
                    let submode = self.latch_buffer[18];
                    let is_xa_audio = (submode & 0x44) == 0x44; // Bit 6 (Real-Time) + Bit 2 (Audio)

                    if xa_enabled && is_xa_audio {
                        let file = self.latch_buffer[16];
                        let channel = self.latch_buffer[17];
                        let filter_en = (self.drive_mode & 0x08) != 0;
                        if !filter_en || (file == self.filter_file && channel == self.filter_channel) {
                            self.decode_xa_sector();
                        }
                        return;
                    }

                    // Se o filtro XA (bit 3) estiver ativo e o setor for Real-Time sem o bit Data (bit 3), ignora
                    let filter_enabled = (self.drive_mode & 0x08) != 0;
                    if filter_enabled && (submode & 0x40) != 0 && (submode & 0x08) == 0 {
                        return;
                    }

                    if self.rx_active {
                        self.sector_buffer.copy_from_slice(&*self.latch_buffer);
                        let whole_sector = (self.drive_mode & (1 << 5)) != 0;
                        self.sector_start_offset = if whole_sector { 12 } else { 24 };
                        self.sector_read_index = self.sector_start_offset;
                        self.data_ready = true;
                    }

                    self.response_fifo.clear();
                    self.response_fifo.push(0x22);
                    self.set_irq(1, irq); // INT1: DataReady
                } else {
                    self.sub_cpu = SubCpuState::Reading(next_rem);
                }
            }
            SubCpuState::PlayingCdDa(remaining) => {
                let next_rem = remaining.saturating_sub(cycles);
                if next_rem == 0 && self.irq_flags == 0 && self.pending_command.is_none() {
                    if let Some(disc) = &mut self.disc {
                        let _ = disc.read_sector_raw(self.seek_lba, &mut self.latch_buffer);
                        // Converte os 2352 bytes de áudio PCM 16-bit estéreo (588 amostras)
                        for chunk in self.latch_buffer.chunks_exact(4) {
                            let l = i16::from_le_bytes([chunk[0], chunk[1]]);
                            let r = i16::from_le_bytes([chunk[2], chunk[3]]);
                            self.audio_samples.push((l, r));
                        }
                    }

                    let abs_frames = self.seek_lba + 150;
                    let m = (abs_frames / (75 * 60)) as u8;
                    let s = ((abs_frames / 75) % 60) as u8;
                    let f = (abs_frames % 75) as u8;
                    let to_bcd = |v: u8| ((v / 10) << 4) | (v % 10);

                    self.seek_lba = self.seek_lba.wrapping_add(1);

                    if (self.drive_mode & 0x04) != 0 {
                        self.response_fifo.clear();
                        self.response_fifo.extend_from_slice(&[
                            0x82,
                            0x01,
                            0x01,
                            to_bcd(m),
                            to_bcd(s) | 0x80,
                            to_bcd(f),
                            0x00,
                            0x00,
                        ]);
                        self.set_irq(1, irq);
                    }

                    self.sub_cpu = SubCpuState::PlayingCdDa(450_000);
                } else {
                    self.sub_cpu = SubCpuState::PlayingCdDa(next_rem);
                }
            }
            SubCpuState::Idle => {}
        }
    }

    pub fn read(&mut self, offset: u32) -> u8 {
        match offset & 3 {
            0 => {
                let mut hsts = self.index & 3;
                if self.param_fifo.is_empty() {
                    hsts |= 1 << 3;
                }
                if self.param_fifo.len() < 16 {
                    hsts |= 1 << 4;
                }
                if !self.response_fifo.is_empty() {
                    hsts |= 1 << 5;
                }
                if self.data_ready {
                    hsts |= 1 << 6;
                }
                if self.pending_command.is_some() {
                    hsts |= 1 << 7;
                }
                hsts
            }
            1 => {
                if !self.response_fifo.is_empty() {
                    self.response_fifo.remove(0)
                } else {
                    0
                }
            }
            2 => {
                let b = self.sector_buffer[self.sector_read_index % 2352];
                self.sector_read_index += 1;
                let whole_sector = (self.drive_mode & (1 << 5)) != 0;
                let end_offset = if whole_sector { 12 + 2340 } else { 24 + 2048 };
                if self.sector_read_index >= end_offset {
                    self.data_ready = false;
                }
                b
            }
            3 => match self.index & 1 {
                0 => self.irq_mask | 0xE0,
                _ => self.irq_flags | 0xE0,
            },
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, val: u8, irq: &mut Interrupts) {
        match offset & 3 {
            0 => {
                self.index = val & 3;
            }
            1 => match self.index {
                0 => self.pending_command = Some((val, 1_500)),
                3 => self.mixer_vol[2] = val, // ATV2
                _ => {}
            },
            2 => match self.index {
                0 => {
                    if self.param_fifo.len() < 16 {
                        self.param_fifo.push(val);
                    }
                }
                1 => {
                    self.irq_mask = val & 0x1F;
                    self.update_irq_line(irq);
                }
                2 => self.mixer_vol[0] = val, // ATV0
                3 => self.mixer_vol[3] = val, // ATV3
                _ => {}
            },
            3 => match self.index {
                0 => {
                    let want_data = (val & 0x80) != 0;
                    if want_data && !self.rx_active {
                        self.sector_buffer.copy_from_slice(&*self.latch_buffer);
                        let whole_sector = (self.drive_mode & (1 << 5)) != 0;
                        self.sector_start_offset = if whole_sector { 12 } else { 24 };
                        self.sector_read_index = self.sector_start_offset;
                        self.data_ready = true;
                    } else if !want_data {
                        self.data_ready = false;
                    }
                    self.rx_active = want_data;
                }
                1 => {
                    self.irq_flags &= !(val & 0x1F);
                    if (val & 0x40) != 0 {
                        self.param_fifo.clear();
                    }
                    self.update_irq_line(irq);
                }
                2 => self.mixer_vol[1] = val, // ATV1
                _ => {}
            },
            _ => {}
        }
    }

    fn execute_command(&mut self, cmd: u8, irq: &mut Interrupts) {
        self.response_fifo.clear();
        let mut stat_running = if self.has_disc { 0x02 } else { 0x10 };
        match self.sub_cpu {
            SubCpuState::Reading(_) => stat_running |= 0x20,
            SubCpuState::PlayingCdDa(_) => stat_running |= 0x80,
            _ => {}
        }

        match cmd {
            0x01 => {
                self.response_fifo.push(stat_running);
                self.set_irq(3, irq);
            }
            0x02 => {
                if self.param_fifo.len() >= 3 {
                    let m = ((self.param_fifo[0] >> 4) * 10 + (self.param_fifo[0] & 0x0F)) as u32;
                    let s = ((self.param_fifo[1] >> 4) * 10 + (self.param_fifo[1] & 0x0F)) as u32;
                    let f = ((self.param_fifo[2] >> 4) * 10 + (self.param_fifo[2] & 0x0F)) as u32;
                    self.seek_lba = ((m * 60 + s) * 75 + f).saturating_sub(150);
                }
                self.param_fifo.clear();
                self.response_fifo.push(stat_running);
                self.set_irq(3, irq);
            }
            0x03 => { // Play CD-DA
                self.param_fifo.clear();
                self.response_fifo.push(0x82);
                self.set_irq(3, irq);
                self.sub_cpu = SubCpuState::PlayingCdDa(450_000);
            }
            0x06 | 0x1B => {
                self.response_fifo.push(0x22);
                self.set_irq(3, irq);
                self.sub_cpu = SubCpuState::Reading(50_000);
            }
            0x07 | 0x08 | 0x09 => {
                self.response_fifo.push(stat_running);
                self.set_irq(3, irq);
                let final_stat = if cmd == 0x08 { 0x00 } else { 0x02 };
                self.sub_cpu = SubCpuState::AwaitingResponse2(final_stat, 8_000);
            }
            0x0A => {
                self.drive_mode = 0x20;
                self.response_fifo.push(stat_running);
                self.set_irq(3, irq);
                self.sub_cpu = SubCpuState::AwaitingResponse2(0x02, 10_000);
            }
            0x0B | 0x0C => {
                self.param_fifo.clear();
                self.response_fifo.push(stat_running);
                self.set_irq(3, irq);
            }
            0x0D => { // SetFilter (File, Channel)
                if self.param_fifo.len() >= 2 {
                    self.filter_file = self.param_fifo[0];
                    self.filter_channel = self.param_fifo[1];
                }
                self.param_fifo.clear();
                self.response_fifo.push(stat_running);
                self.set_irq(3, irq);
            }
            0x0E => {
                if !self.param_fifo.is_empty() {
                    self.drive_mode = self.param_fifo[0];
                }
                self.param_fifo.clear();
                self.response_fifo.push(stat_running);
                self.set_irq(3, irq);
            }
            0x0F => {
                self.response_fifo.extend_from_slice(&[
                    stat_running,
                    self.drive_mode,
                    0,
                    self.filter_file,
                    self.filter_channel,
                ]);
                self.set_irq(3, irq);
            }
            0x10 => {
                let hdr = &self.latch_buffer[12..20];
                self.response_fifo.extend_from_slice(hdr);
                self.set_irq(3, irq);
            }
            0x11 => {
                let abs_frames = self.seek_lba + 150;
                let m = (abs_frames / (75 * 60)) as u8;
                let s = ((abs_frames / 75) % 60) as u8;
                let f = (abs_frames % 75) as u8;
                let to_bcd = |v: u8| ((v / 10) << 4) | (v % 10);
                self.response_fifo.extend_from_slice(&[
                    0x01,
                    0x01,
                    to_bcd(m),
                    to_bcd(s.saturating_sub(2)),
                    to_bcd(f),
                    to_bcd(m),
                    to_bcd(s),
                    to_bcd(f),
                ]);
                self.set_irq(3, irq);
            }
            0x13 => {
                self.response_fifo.extend_from_slice(&[stat_running, 0x01, 0x01]);
                self.set_irq(3, irq);
            }
            0x14 => {
                self.param_fifo.clear();
                self.response_fifo.extend_from_slice(&[stat_running, 0x00, 0x02]);
                self.set_irq(3, irq);
            }
            0x15 | 0x16 => {
                self.response_fifo.push(stat_running | 0x40);
                self.set_irq(3, irq);
                self.sub_cpu = SubCpuState::AwaitingResponse2(0x02, 8_000);
            }
            0x19 => {
                let sub_cmd = if !self.param_fifo.is_empty() {
                    self.param_fifo.remove(0)
                } else {
                    0
                };
                self.param_fifo.clear();
                match sub_cmd {
                    0x20 => {
                        self.response_fifo.extend_from_slice(&[0x94, 0x09, 0x19, 0xC0]);
                        self.set_irq(3, irq);
                    }
                    _ => {
                        self.response_fifo.push(stat_running);
                        self.set_irq(3, irq);
                    }
                }
            }
            0x1A => {
                self.response_fifo.push(stat_running);
                self.set_irq(3, irq);
                self.sub_cpu = SubCpuState::AwaitingGetId(10_000);
            }
            0x1E => {
                self.response_fifo.push(stat_running);
                self.set_irq(3, irq);
                self.sub_cpu = SubCpuState::AwaitingResponse2(0x02, 10_000);
            }
            _ => {
                self.param_fifo.clear();
                self.response_fifo.push(stat_running);
                self.set_irq(3, irq);
            }
        }
    }

    pub fn dma_read_word(&mut self) -> u32 {
        let idx = self.sector_read_index;
        let b0 = self.sector_buffer[idx % 2352] as u32;
        let b1 = self.sector_buffer[(idx + 1) % 2352] as u32;
        let b2 = self.sector_buffer[(idx + 2) % 2352] as u32;
        let b3 = self.sector_buffer[(idx + 3) % 2352] as u32;

        self.sector_read_index += 4;
        let whole_sector = (self.drive_mode & (1 << 5)) != 0;
        let end_offset = if whole_sector { 12 + 2340 } else { 24 + 2048 };
        if self.sector_read_index >= end_offset {
            self.data_ready = false;
        }
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    /// Decodifica um setor de áudio Mode 2 Form 2 CD-XA ADPCM (18 Sound Groups de 128 bytes)
    /// e faz o resampling de 37.8 kHz / 18.9 kHz para 44.1 kHz
    fn decode_xa_sector(&mut self) {
        const POS_XA: [i32; 4] = [0, 60, 115, 98];
        const NEG_XA: [i32; 4] = [0, 0, -52, -55];

        let coding = self.latch_buffer[19];
        let is_stereo = (coding & 0x03) == 1;
        let is_half_rate = ((coding >> 2) & 0x03) == 1; // 0 = 37800 Hz, 1 = 18900 Hz
        let is_8bit = ((coding >> 4) & 0x03) == 1;

        if is_8bit {
            return; // 99.9% dos jogos de PS1 usam 4-bit XA
        }

        self.xa_pcm_l.clear();
        self.xa_pcm_r.clear();

        // Os 18 Sound Groups começam no byte 24 do setor bruto
        for sg in 0..18 {
            let sg_base = 24 + sg * 128;
            if sg_base + 128 > 2352 {
                break;
            }
            let group = &self.latch_buffer[sg_base..sg_base + 128];

            for su in 0..8 {
                let header = group[4 + su];
                let mut shift = (header & 0x0F) as u32;
                if shift > 12 {
                    shift = 9;
                }
                let filter = (((header >> 4) & 0x03) as usize).min(3);
                let f0 = POS_XA[filter];
                let f1 = NEG_XA[filter];

                let is_right_chan = is_stereo && (su & 1) != 0;

                for w in 0..28 {
                    let byte = group[16 + w * 4 + (su / 2)];
                    let nibble = if (su & 1) == 0 {
                        byte & 0x0F
                    } else {
                        byte >> 4
                    };

                    let signed_16 = ((nibble as i16) << 12) as i32;
                    let sample_in = signed_16 >> shift;

                    let (old, older) = if is_right_chan {
                        self.xa_hist_r
                    } else {
                        self.xa_hist_l
                    };

                    let predicted = sample_in + ((old * f0 + older * f1 + 32) >> 6);
                    let clamped = predicted.clamp(-32768, 32767);

                    if is_right_chan {
                        self.xa_hist_r = (clamped, old);
                        self.xa_pcm_r.push(clamped as i16);
                    } else {
                        self.xa_hist_l = (clamped, old);
                        self.xa_pcm_l.push(clamped as i16);
                        if !is_stereo {
                            self.xa_pcm_r.push(clamped as i16);
                        }
                    }
                }
            }
        }

        // Reamostragem de 37.800 Hz (ou 18.900 Hz) para 44.100 Hz (proporção 6 -> 7 ou 3 -> 7)
        let len = self.xa_pcm_l.len().min(self.xa_pcm_r.len());
        let step_num = if is_half_rate { 3 } else { 6 };
        let out_len = (len * 7) / step_num;

        let v_ll = self.mixer_vol[0] as i32;
        let v_lr = self.mixer_vol[1] as i32;
        let v_rr = self.mixer_vol[2] as i32;
        let v_rl = self.mixer_vol[3] as i32;

        for i in 0..out_len {
            let src_idx = (i * step_num) / 7;
            let l_in = self.xa_pcm_l[src_idx.min(len - 1)] as i32;
            let r_in = self.xa_pcm_r[src_idx.min(len - 1)] as i32;

            let mixed_l = ((l_in * v_ll + r_in * v_rl) >> 7).clamp(-32768, 32767) as i16;
            let mixed_r = ((r_in * v_rr + l_in * v_lr) >> 7).clamp(-32768, 32767) as i16;
            self.audio_samples.push((mixed_l, mixed_r));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cdrom_index_and_status() {
        let mut cd = CdRom::new();
        assert_eq!(cd.read(0) & 0x18, 0x18);

        let mut irq = Interrupts::new();
        cd.write(0, 1, &mut irq);
        assert_eq!(cd.read(0) & 3, 1);
    }

    #[test]
    fn test_cdrom_get_stat_command() {
        let mut cd = CdRom::new();
        let mut irq = Interrupts::new();
        irq.mask = 0xFFFF;

        cd.write(0, 1, &mut irq);
        cd.write(2, 0x1F, &mut irq);

        cd.write(0, 0, &mut irq);
        cd.write(1, 0x01, &mut irq);
        cd.tick(2_000, &mut irq);

        assert_eq!(cd.irq_flags & 7, 3);
        assert!(irq.is_active());
        assert_eq!(cd.read(1), 0x02);
    }

    #[test]
    fn test_cdrom_version_test_command() {
        let mut cd = CdRom::new();
        let mut irq = Interrupts::new();

        cd.write(0, 0, &mut irq);
        cd.write(2, 0x20, &mut irq);
        cd.write(1, 0x19, &mut irq);
        cd.tick(2_000, &mut irq);

        assert_eq!(cd.read(1), 0x94);
        assert_eq!(cd.read(1), 0x09);
        assert_eq!(cd.read(1), 0x19);
        assert_eq!(cd.read(1), 0xC0);
    }

    #[test]
    fn test_cdrom_async_second_response() {
        let mut cd = CdRom::new();
        let mut irq = Interrupts::new();
        irq.mask = 0xFFFF;

        cd.write(0, 1, &mut irq);
        cd.write(2, 0x1F, &mut irq);

        cd.write(0, 0, &mut irq);
        cd.write(1, 0x09, &mut irq);
        cd.tick(2_000, &mut irq);

        assert_eq!(cd.irq_flags & 7, 3);

        cd.write(0, 1, &mut irq);
        cd.write(3, 0x1F, &mut irq);
        assert_eq!(cd.irq_flags & 7, 0);

        cd.tick(10_000, &mut irq);
        assert_eq!(cd.irq_flags & 7, 2);
    }
}
