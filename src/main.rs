#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// src/main.rs

#![allow(dead_code)]
#![allow(unused_variables)]

mod bios;
mod bus;
mod cdrom;
mod controller;
mod cpu;
mod disc;
mod dma;
mod exe;
mod gpu;
mod gte;
mod instruction;
mod interrupt;
mod mdec;
mod spu;
mod timers;
mod monitor;
mod prof;
mod hub;
mod pgxp;
mod memcard;
mod savestate;

use std::env;
use std::fs;
use std::path::Path;
use bus::Bus;
use controller::Button;
use cpu::Cpu;
use disc::Disc;
use exe::ExeLoader;
use gpu::{TV_HEIGHT, TV_WIDTH};
use monitor::PerformanceMonitor;
use interrupt::InterruptType;
use minifb::{Key, Window, WindowOptions};

const SCANLINES_PER_FRAME: usize = 263;
const CYCLES_PER_SCANLINE: usize = 33_868_800 / (60 * SCANLINES_PER_FRAME);

// =============================================================================
// Integração Nativa Windows: Controle de Xbox (XInput) e Áudio 44.1kHz (WinMM)
// =============================================================================

#[cfg(target_os = "windows")]
mod win_platform {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct XInputGamepad {
        pub w_buttons: u16,
        pub b_left_trigger: u8,
        pub b_right_trigger: u8,
        pub s_thumb_lx: i16,
        pub s_thumb_ly: i16,
        pub s_thumb_rx: i16,
        pub s_thumb_ry: i16,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct XInputState {
        pub dw_packet_number: u32,
        pub gamepad: XInputGamepad,
    }

    #[repr(C)]
    struct WaveFormatEx {
        w_format_tag: u16,
        n_channels: u16,
        n_samples_per_sec: u32,
        n_avg_bytes_per_sec: u32,
        n_block_align: u16,
        w_bits_per_sample: u16,
        cb_size: u16,
    }

    #[repr(C)]
    struct WaveHdr {
        lp_data: *mut u8,
        dw_buffer_length: u32,
        dw_bytes_recorded: u32,
        dw_user: usize,
        dw_flags: u32,
        dw_loops: u32,
        lp_next: *mut WaveHdr,
        reserved: usize,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct XInputVibration {
        pub w_left_motor_speed: u16,
        pub w_right_motor_speed: u16,
    }

    #[link(name = "xinput9_1_0")]
    extern "system" {
        fn XInputGetState(dw_user_index: u32, p_state: *mut XInputState) -> u32;
        fn XInputSetState(dw_user_index: u32, p_vibration: *const XInputVibration) -> u32;
    }

    pub fn set_xbox_rumble(left_motor: u16, right_motor: u16) {
        let vib = XInputVibration {
            w_left_motor_speed: left_motor,
            w_right_motor_speed: right_motor,
        };
        unsafe {
            XInputSetState(0, &vib);
        }
    }

    #[repr(C)]
    #[derive(Default)]
    struct JoyInfoEx {
        dw_size: u32,
        dw_flags: u32,
        dw_xpos: u32,
        dw_ypos: u32,
        dw_zpos: u32,
        dw_rpos: u32,
        dw_upos: u32,
        dw_vpos: u32,
        dw_buttons: u32,
        dw_button_number: u32,
        dw_pov: u32,
        dw_reserved1: u32,
        dw_reserved2: u32,
    }

    #[link(name = "winmm")]
    extern "system" {
        fn timeBeginPeriod(u_period: u32) -> u32;
        fn joyGetPosEx(u_joy_id: u32, pji: *mut JoyInfoEx) -> u32;
        fn waveOutOpen(
            phwo: *mut *mut c_void,
            u_device_id: u32,
            pwfx: *const WaveFormatEx,
            dw_callback: usize,
            dw_instance: usize,
            fdw_open: u32,
        ) -> u32;
        fn waveOutPrepareHeader(hwo: *mut c_void, pwh: *mut WaveHdr, cbwh: u32) -> u32;
        fn waveOutWrite(hwo: *mut c_void, pwh: *mut WaveHdr, cbwh: u32) -> u32;
    }

    use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

    // Cache da porta conectada: -1 = nenhuma, 0..3 = XInput, 10..13 = WinMM Joy
    static ACTIVE_PORT: AtomicI32 = AtomicI32::new(-1);
    static SCAN_COOLDOWN: AtomicU32 = AtomicU32::new(0);

    pub fn poll_xbox_controller() -> Option<XInputGamepad> {
        let port = ACTIVE_PORT.load(Ordering::Relaxed);

        // 1. Se já sabemos qual porta está conectada, lê APENAS ela (custo zero!)
        if (0..4).contains(&port) {
            let mut state = XInputState::default();
            let res = unsafe { XInputGetState(port as u32, &mut state) };
            if res == 0 {
                return Some(state.gamepad);
            }
            ACTIVE_PORT.store(-1, Ordering::Relaxed);
        } else if (10..14).contains(&port) {
            let joy_id = (port - 10) as u32;
            if let Some(gp) = read_winmm_joy(joy_id) {
                return Some(gp);
            }
            ACTIVE_PORT.store(-1, Ordering::Relaxed);
        }

        // 2. Se nenhum controle estiver conectado, só escaneia as portas a cada 120 frames (~2 segundos)!
        let cd = SCAN_COOLDOWN.fetch_add(1, Ordering::Relaxed);
        if cd % 120 != 0 {
            return None;
        }

        for p in 0..4 {
            let mut state = XInputState::default();
            if unsafe { XInputGetState(p, &mut state) } == 0 {
                ACTIVE_PORT.store(p as i32, Ordering::Relaxed);
                return Some(state.gamepad);
            }
        }

        for joy_id in 0..2 {
            if let Some(gp) = read_winmm_joy(joy_id) {
                ACTIVE_PORT.store(10 + joy_id as i32, Ordering::Relaxed);
                return Some(gp);
            }
        }

        None
    }

    fn read_winmm_joy(joy_id: u32) -> Option<XInputGamepad> {
        let mut ji = JoyInfoEx {
            dw_size: std::mem::size_of::<JoyInfoEx>() as u32,
            dw_flags: 0xFF,
            ..Default::default()
        };
        if unsafe { joyGetPosEx(joy_id, &mut ji) } != 0 {
            return None;
        }

        let mut gp = XInputGamepad::default();
        gp.s_thumb_lx = (ji.dw_xpos as i32 - 32768).clamp(-32768, 32767) as i16;
        gp.s_thumb_ly = (32768 - ji.dw_ypos as i32).clamp(-32768, 32767) as i16;

        if ji.dw_pov < 36000 {
            if ji.dw_pov <= 4500 || ji.dw_pov >= 31500 { gp.w_buttons |= 0x0001; }
            if (4500..=13500).contains(&ji.dw_pov) { gp.w_buttons |= 0x0008; }
            if (13500..=22500).contains(&ji.dw_pov) { gp.w_buttons |= 0x0002; }
            if (22500..=31500).contains(&ji.dw_pov) { gp.w_buttons |= 0x0004; }
        }

        if (ji.dw_buttons & (1 << 0)) != 0 { gp.w_buttons |= 0x1000; }
        if (ji.dw_buttons & (1 << 1)) != 0 { gp.w_buttons |= 0x2000; }
        if (ji.dw_buttons & (1 << 2)) != 0 { gp.w_buttons |= 0x4000; }
        if (ji.dw_buttons & (1 << 3)) != 0 { gp.w_buttons |= 0x8000; }
        if (ji.dw_buttons & (1 << 4)) != 0 { gp.w_buttons |= 0x0100; }
        if (ji.dw_buttons & (1 << 5)) != 0 { gp.w_buttons |= 0x0200; }
        if (ji.dw_buttons & (1 << 6)) != 0 { gp.w_buttons |= 0x0020; }
        if (ji.dw_buttons & (1 << 7)) != 0 { gp.w_buttons |= 0x0010; }

        Some(gp)
    }

    const NUM_AUDIO_BUFFERS: usize = 8;
    const BUFFER_SAMPLES: usize = 2048; // 1024 frames estéreo (~23.2ms por bloco, zero engasgos)
    const WHDR_DONE: u32 = 0x0000_0001;

    pub struct WinAudioPlayer {
        hwo: *mut c_void,
        buffers: Vec<Box<[i16; BUFFER_SAMPLES]>>,
        headers: Vec<Box<WaveHdr>>,
        cur_buf: usize,
        pending_queue: std::collections::VecDeque<i16>,
        last_l: i16,
        last_r: i16,
        started: bool,
    }

    impl WinAudioPlayer {
        pub fn new() -> Option<Self> {
            unsafe {
                timeBeginPeriod(1);
            }

            let wfx = WaveFormatEx {
                w_format_tag: 1,
                n_channels: 2,
                n_samples_per_sec: 44_100,
                n_avg_bytes_per_sec: 44_100 * 4,
                n_block_align: 4,
                w_bits_per_sample: 16,
                cb_size: 0,
            };

            let mut hwo: *mut c_void = null_mut();
            let res = unsafe { waveOutOpen(&mut hwo, 0xFFFF_FFFF, &wfx, 0, 0, 0) };
            if res != 0 || hwo.is_null() {
                return None;
            }

            let mut buffers = Vec::with_capacity(NUM_AUDIO_BUFFERS);
            let mut headers = Vec::with_capacity(NUM_AUDIO_BUFFERS);

            for _ in 0..NUM_AUDIO_BUFFERS {
                let mut buf = Box::new([0i16; BUFFER_SAMPLES]);
                let mut hdr = Box::new(WaveHdr {
                    lp_data: buf.as_mut_ptr() as *mut u8,
                    dw_buffer_length: (BUFFER_SAMPLES * 2) as u32,
                    dw_bytes_recorded: 0,
                    dw_user: 0,
                    dw_flags: WHDR_DONE,
                    dw_loops: 0,
                    lp_next: null_mut(),
                    reserved: 0,
                });

                unsafe {
                    waveOutPrepareHeader(hwo, &mut *hdr, std::mem::size_of::<WaveHdr>() as u32);
                }
                hdr.dw_flags |= WHDR_DONE;

                buffers.push(buf);
                headers.push(hdr);
            }

            Some(Self {
                hwo,
                buffers,
                headers,
                cur_buf: 0,
                pending_queue: std::collections::VecDeque::with_capacity(16384),
                last_l: 0,
                last_r: 0,
                started: false,
            })
        }

        /// Retorna quantos blocos ainda estão tocando na placa de som
        pub fn queued_blocks(&self) -> usize {
            self.headers
                .iter()
                .filter(|h| (h.dw_flags & WHDR_DONE) == 0)
                .count()
        }

        pub fn submit_samples(&mut self, samples: &[i16]) {
            let num_frames = samples.len() / 2;
            if num_frames < 2 {
                return;
            }

            // Conta amostras totais (na fila em RAM + nos buffers ativos do Windows)
            let active_hw_samples = self.queued_blocks() * BUFFER_SAMPLES;
            let total_buffered = self.pending_queue.len() + active_hw_samples;

            // Ajuste suave de pitch (apenas +/- 0.4%) para nunca dar artefato audível
            let ratio_fp: usize = if total_buffered < 4096 {
                996 // Precisa de um pouquinho mais de amostras
            } else if total_buffered > 8192 {
                1004 // Buffer cheio demais, consome levemente mais rápido
            } else {
                1000
            };

            let out_frames = (num_frames * 1000) / ratio_fp;
            if self.pending_queue.len() < 12288 {
                for i in 0..out_frames {
                    let pos_fp = (i * ratio_fp * 256) / 1000;
                    let idx0 = (pos_fp >> 8).min(num_frames - 1);
                    let idx1 = (idx0 + 1).min(num_frames - 1);
                    let frac = (pos_fp & 0xFF) as i32;

                    let l0 = samples[idx0 * 2] as i32;
                    let r0 = samples[idx0 * 2 + 1] as i32;
                    let l1 = samples[idx1 * 2] as i32;
                    let r1 = samples[idx1 * 2 + 1] as i32;

                    let l_interp = (l0 + (((l1 - l0) * frac) >> 8)) as i16;
                    let r_interp = (r0 + (((r1 - r0) * frac) >> 8)) as i16;

                    self.pending_queue.push_back(l_interp);
                    self.pending_queue.push_back(r_interp);
                }
            }

            // Pré-carrega 2 blocos (~46ms) antes de começar a enviar, evitando engasgo inicial
            if !self.started {
                if self.pending_queue.len() < BUFFER_SAMPLES * 2 {
                    return;
                }
                self.started = true;
            }

            while self.pending_queue.len() >= BUFFER_SAMPLES {
                let hdr = &mut self.headers[self.cur_buf];
                if (hdr.dw_flags & WHDR_DONE) == 0 {
                    break;
                }

                let buf = &mut self.buffers[self.cur_buf];
                for pair in buf.chunks_exact_mut(2) {
                    if let (Some(l), Some(r)) = (self.pending_queue.pop_front(), self.pending_queue.pop_front()) {
                        self.last_l = l;
                        self.last_r = r;
                    }
                    pair[0] = self.last_l;
                    pair[1] = self.last_r;
                }

                hdr.dw_buffer_length = (BUFFER_SAMPLES * 2) as u32;
                hdr.dw_flags &= !WHDR_DONE;

                unsafe {
                    waveOutWrite(self.hwo, &mut **hdr, std::mem::size_of::<WaveHdr>() as u32);
                }

                self.cur_buf = (self.cur_buf + 1) % NUM_AUDIO_BUFFERS;
            }
        }
    }
}

fn run_one_frame(cpu: &mut Cpu, bus: &mut Bus, clock_mult: f32) {
    const BATCH_INSTRS: usize = 16;
    const BATCH_CYCLES: u32 = 32;
    let batches_per_line = ((CYCLES_PER_SCANLINE as f32 * clock_mult) / (BATCH_CYCLES as f32)).max(1.0) as usize;

    for line in 0..SCANLINES_PER_FRAME {
        if line < 240 && (line & 1) != 0 {
            bus.gpu.status |= 1 << 31;
        } else {
            bus.gpu.status &= !(1 << 31);
        }

        if line == 240 {
            bus.irq.trigger(InterruptType::VBlank);
        }

        for _ in 0..batches_per_line {
            for _ in 0..BATCH_INSTRS {
                cpu.step_core(bus);
            }
            Cpu::tick_peripherals(bus, BATCH_CYCLES);
        }
    }
}

/// Executa o emulador sem janela e mede o custo por frame (CPU vs GPU vs saída de vídeo).
fn run_benchmark(args: &[String], bios_bytes: &[u8]) {
    let path = &args[0];
    let frames: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1800);
    let has = |k: &str| args.iter().any(|a| a.eq_ignore_ascii_case(k));

    let mut cpu = Cpu::new();
    let mut bus = Bus::new(bios_bytes.to_vec());
    if !boot_game(path, bios_bytes, &mut cpu, &mut bus) {
        println!("Falha ao carregar {}", path);
        return;
    }
    bus.gpu.config.render_mode = if has("sw") { gpu::RenderMode::Software } else { gpu::RenderMode::Hardware };
    bus.gpu.config.pgxp_enabled = has("pgxp");
    bus.gpu.config.perspective_correct = has("pgxp");
    bus.gpu.config.bilinear_filter = has("bilinear");
    bus.gpu.perf_enabled = true;
    unsafe { prof::ENABLED = true; }
    // Calibra o TSC contra o relógio do sistema
    let tsc_per_ms = {
        let a = prof::now();
        let s = std::time::Instant::now();
        while s.elapsed().as_millis() < 50 {}
        (prof::now() - a) as f64 / s.elapsed().as_secs_f64() / 1000.0
    };
    let mut worst_frames: Vec<(f64, usize)> = Vec::new();

    let mut display = [0u32; TV_WIDTH * TV_HEIGHT];
    let mut frame_ms: Vec<f64> = Vec::with_capacity(frames);
    let (mut total_emu, mut total_gpu, mut total_out) = (0f64, 0f64, 0f64);
    let start = std::time::Instant::now();

    for _ in 0..frames {
        bus.gpu.perf_gp0_ns = 0;
        let before = unsafe { prof::COUNTERS };
        let t0 = std::time::Instant::now();
        run_one_frame(&mut cpu, &mut bus, 1.0);
        let t1 = std::time::Instant::now();
        bus.gpu.render_to_display_buffer(&mut display);
        bus.spu.audio_out.clear();
        let t2 = std::time::Instant::now();

        let emu = (t1 - t0).as_secs_f64() * 1000.0;
        let gpu_ms = bus.gpu.perf_gp0_ns as f64 / 1e6;
        let out = (t2 - t1).as_secs_f64() * 1000.0;
        total_emu += emu;
        total_gpu += gpu_ms;
        total_out += out;
        frame_ms.push(emu + out);
        if emu + out > 16.67 {
            worst_frames.push((emu + out, frame_ms.len() - 1));
            if emu + out > 30.0 {
                let after = unsafe { prof::COUNTERS };
                let d = |i: usize| (after[i] - before[i]) as f64 / tsc_per_ms;
                println!("  frame {} = {:.1}ms | gpu {:.1} dma {:.1} cdrom {:.1} spu {:.1} timers {:.1}",
                    frame_ms.len() - 1, emu + out, gpu_ms, d(prof::DMA), d(prof::CDROM), d(prof::SPU), d(prof::TIMERS));
            }
        }
    }

    let wall = start.elapsed().as_secs_f64();
    let n = frames as f64;
    let mut sorted = frame_ms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p99 = sorted[((sorted.len() as f64 * 0.99) as usize).min(sorted.len() - 1)];
    let worst = *sorted.last().unwrap();
    let over = frame_ms.iter().filter(|&&m| m > 16.67).count();

    println!("=== BENCHMARK {} frames ({:.1}s reais) ===", frames, wall);
    println!("Media por frame : {:.2} ms  (~{:.0} FPS possiveis)", (total_emu + total_out) / n, 1000.0 / ((total_emu + total_out) / n));
    println!("  CPU+perifericos: {:.2} ms", (total_emu - total_gpu) / n);
    println!("  GPU (desenho)  : {:.2} ms", total_gpu / n);
    println!("  Saida de video : {:.2} ms", total_out / n);
    println!("P99 {:.2} ms | Pior {:.2} ms | Frames acima de 16.67ms: {}", p99, worst, over);
    let c = unsafe { prof::COUNTERS };
    for i in 1..7 {
        println!("  [{:>6}] {:.3} ms/frame", prof::NAMES[i], c[i] as f64 / tsc_per_ms / n);
    }
    worst_frames.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let list: Vec<String> = worst_frames.iter().take(10).map(|(ms, f)| format!("#{}={:.1}ms", f, ms)).collect();
    println!("Piores frames: {}", list.join(" "));
}

fn boot_game(file_path: &str, bios_bytes: &[u8], cpu: &mut Cpu, bus: &mut Bus) -> bool {
    *cpu = Cpu::new();
    *bus = Bus::new(bios_bytes.to_vec());

    let path = Path::new(file_path);
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();

    match ext.as_str() {
        "cue" | "bin" | "iso" => {
            println!("Carregando disco no drive: {}", file_path);
            if let Ok(disc) = Disc::open(file_path) {
                bus.cdrom.disc = Some(disc);
                bus.cdrom.has_disc = true;
                true
            } else {
                println!("Erro ao abrir imagem de CD: {}", file_path);
                false
            }
        }
        "exe" | "psx" => {
            println!("Inicializando Kernel ate 0x80030000 para injetar EXE...");
            while cpu.pc != 0x8003_0000 {
                cpu.step(bus);
            }
            ExeLoader::load_and_boot(file_path, cpu, bus).is_ok()
        }
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppMode {
    Hub,
    InGame,
}

#[cfg(target_os = "windows")]
fn apply_window_mode(window: &minifb::Window, mode: hub::DisplayMode) {
    let hwnd_ptr = window.get_window_handle();
    if hwnd_ptr.is_null() {
        return;
    }

    unsafe {
        extern "system" {
            fn SetWindowLongPtrA(hwnd: *mut std::ffi::c_void, index: i32, new_long: isize) -> isize;
            fn SetWindowPos(
                hwnd: *mut std::ffi::c_void,
                insert_after: *mut std::ffi::c_void,
                x: i32,
                y: i32,
                cx: i32,
                cy: i32,
                flags: u32,
            ) -> i32;
            fn GetSystemMetrics(index: i32) -> i32;
            fn ShowWindow(hwnd: *mut std::ffi::c_void, command: i32) -> i32;
        }

        let screen_w = GetSystemMetrics(0); // SM_CXSCREEN
        let screen_h = GetSystemMetrics(1); // SM_CYSCREEN

        const GWL_STYLE: i32 = -16;
        const WS_OVERLAPPEDWINDOW: isize = 0x00CF0000;
        const WS_POPUP: isize = 0x80000000;
        const WS_VISIBLE: isize = 0x10000000;
        const SWP_FRAMECHANGED: u32 = 0x0020;
        const SWP_SHOWWINDOW: u32 = 0x0040;
        const SWP_NOZORDER: u32 = 0x0004;
        const SW_RESTORE: i32 = 9;
        const HWND_TOP: *mut std::ffi::c_void = std::ptr::null_mut();
        const HWND_TOPMOST: *mut std::ffi::c_void = usize::MAX as *mut std::ffi::c_void;

        match mode {
            hub::DisplayMode::Windowed => {
                ShowWindow(hwnd_ptr, SW_RESTORE);
                SetWindowLongPtrA(hwnd_ptr, GWL_STYLE, WS_OVERLAPPEDWINDOW | WS_VISIBLE);
                let win_w = 1024;
                let win_h = 576;
                let win_x = ((screen_w - win_w) / 2).max(0);
                let win_y = ((screen_h - win_h) / 2).max(0);
                SetWindowPos(
                    hwnd_ptr,
                    HWND_TOP,
                    win_x,
                    win_y,
                    win_w,
                    win_h,
                    SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOZORDER,
                );
            }
            hub::DisplayMode::Borderless => {
                ShowWindow(hwnd_ptr, SW_RESTORE);
                SetWindowLongPtrA(hwnd_ptr, GWL_STYLE, WS_POPUP | WS_VISIBLE);
                SetWindowPos(
                    hwnd_ptr,
                    HWND_TOP,
                    0,
                    0,
                    screen_w,
                    screen_h,
                    SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOZORDER,
                );
            }
            hub::DisplayMode::Fullscreen => {
                SetWindowLongPtrA(hwnd_ptr, GWL_STYLE, WS_POPUP | WS_VISIBLE);
                SetWindowPos(
                    hwnd_ptr,
                    HWND_TOPMOST,
                    0,
                    0,
                    screen_w,
                    screen_h,
                    SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOZORDER,
                );
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_window_mode(_window: &minifb::Window, _mode: hub::DisplayMode) {}

fn blit_ps1_to_canvas(display: &[u32; TV_WIDTH * TV_HEIGHT], canvas: &mut [u32], aspect: hub::AspectMode) {
    match aspect {
        hub::AspectMode::Classic4x3 => {
            const TARGET_W: usize = 384;
            const OFFSET_X: usize = (hub::CANVAS_WIDTH - TARGET_W) / 2; // 64 pixels

            for y in 0..hub::CANVAS_HEIGHT {
                let row_start = y * hub::CANVAS_WIDTH;
                canvas[row_start..row_start + OFFSET_X].fill(0x06090E);
                canvas[row_start + OFFSET_X + TARGET_W..row_start + hub::CANVAS_WIDTH].fill(0x06090E);

                canvas[row_start + OFFSET_X - 1] = 0x1E2B42;
                canvas[row_start + OFFSET_X + TARGET_W] = 0x1E2B42;

                let src_y = (y * TV_HEIGHT) / hub::CANVAS_HEIGHT;
                let src_row = src_y * TV_WIDTH;
                let dst_row = row_start + OFFSET_X;

                for x in 0..TARGET_W {
                    let src_x = (x * TV_WIDTH) / TARGET_W;
                    canvas[dst_row + x] = display[src_row + src_x];
                }
            }
        }
        hub::AspectMode::WidescreenHack | hub::AspectMode::WidescreenStretch => {
            for y in 0..hub::CANVAS_HEIGHT {
                let src_y = (y * TV_HEIGHT) / hub::CANVAS_HEIGHT;
                let src_row = src_y * TV_WIDTH;
                let dst_row = y * hub::CANVAS_WIDTH;

                for x in 0..hub::CANVAS_WIDTH {
                    let src_x = (x * TV_WIDTH) / hub::CANVAS_WIDTH;
                    canvas[dst_row + x] = display[src_row + src_x];
                }
            }
        }
    }
}

fn find_ps1_bios() -> Option<(Vec<u8>, String)> {
    let bios_names = [
        "SCPH1001.BIN", "scph1001.bin",
        "SCPH-1001.bin", "SCPH-1001.BIN",
        "SCPH7001.BIN", "scph7001.bin",
        "SCPH5501.BIN", "scph5501.bin",
        "SCPH1000.BIN", "scph1000.bin",
        "SCPH1002.BIN", "scph1002.bin",
        "ps-22j.bin", "ps-30a.bin", "ps-30e.bin",
        "bios.bin", "BIOS.BIN"
    ];

    let mut search_dirs = Vec::new();

    // 1. Diretório onde está o executável .exe
    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            search_dirs.push(exe_dir.to_path_buf());
            search_dirs.push(exe_dir.join("bios"));

            // Se rodar de target/release ou target/debug, procura na raiz do projeto (../../..)
            if let Some(p1) = exe_dir.parent() {
                if let Some(project_root) = p1.parent() {
                    search_dirs.push(project_root.to_path_buf());
                    search_dirs.push(project_root.join("bios"));
                }
            }
        }
    }

    // 2. Diretório atual de trabalho
    if let Ok(cur) = env::current_dir() {
        search_dirs.push(cur.clone());
        search_dirs.push(cur.join("bios"));
    }

    // 3. Diretório oficial de BIOS e ISOs em Documentos
    let doc_bios = hub::get_documents_bios_dir();
    search_dirs.push(doc_bios.clone());
    let doc_isos = hub::get_documents_isos_dir();
    if let Some(meu_psx_doc) = doc_isos.parent() {
        search_dirs.push(meu_psx_doc.to_path_buf());
        search_dirs.push(meu_psx_doc.join("bios"));
    }

    // 1º passo: busca pelos nomes padrão de BIOS
    for dir in &search_dirs {
        for name in &bios_names {
            let p = dir.join(name);
            if p.is_file() {
                if let Ok(bytes) = fs::read(&p) {
                    if bytes.len() == 512 * 1024 {
                        let path_str = p.display().to_string();
                        return Some((bytes, path_str));
                    }
                }
            }
        }
    }

    // 2º passo: busca por qualquer arquivo com exatamente 512 KB (524.288 bytes)
    for dir in &search_dirs {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    if let Ok(meta) = fs::metadata(&p) {
                        if meta.len() == 524288 {
                            if let Ok(bytes) = fs::read(&p) {
                                let path_str = p.display().to_string();
                                return Some((bytes, path_str));
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

fn main() {
    #[cfg(target_os = "windows")]
    unsafe {
        extern "system" {
            fn timeBeginPeriod(u_period: u32) -> u32;
        }
        timeBeginPeriod(1);
    }
    println!("========================================================");
    println!("        MEU EMULADOR DE PLAYSTATION 1 (PSX)            ");
    println!("         ARCADE LIBRARY HUB & RENDERIZADOR HD           ");
    println!("========================================================");

    let (bios_bytes, bios_path_display) = if let Some((bytes, p)) = find_ps1_bios() {
        println!("BIOS Carregada : {}", p);
        (bytes, p)
    } else {
        println!("AVISO: BIOS SCPH1001.BIN nao encontrada! Inicializando modo Hub de navegacao.");
        (vec![0u8; 512 * 1024], "Nao encontrada".to_string())
    };

    let mut bus = Bus::new(bios_bytes.clone());
    let mut cpu = Cpu::new();

    if let Some(meta) = bus.bios.metadata {
        println!("BIOS Detectada : {} (Regiao: {:?})", meta.name, meta.region);
    }

    // Modo benchmark sem janela: meu_psx.exe --bench <jogo.cue> [frames] [sw] [pgxp] [bilinear]
    {
        let args: Vec<String> = env::args().collect();
        if args.len() > 2 && args[1] == "--bench" {
            run_benchmark(&args[2..], &bios_bytes);
            return;
        }
    }

    // Escaneia jogos para o Hub Arcade
    let mut games = hub::scan_games();
    let mut hub_state = hub::HubState::new(games.len());
    let cover_loader = hub::AsyncCoverLoader::new();
    let mut current_game_path = String::new();
    let mut current_game_title = String::new();

    let args: Vec<String> = env::args().collect();
    let mut app_mode = if args.len() > 1 {
        let path = args[1].clone();
        if boot_game(&path, &bios_bytes, &mut cpu, &mut bus) {
            current_game_path = path.clone();
            current_game_title = hub::sanitize_game_title(
                Path::new(&path).file_stem().unwrap_or_default().to_str().unwrap_or(&path)
            );
            AppMode::InGame
        } else {
            AppMode::Hub
        }
    } else {
        AppMode::Hub
    };

    #[cfg(target_os = "windows")]
    let mut audio_player = {
        let player = win_platform::WinAudioPlayer::new();
        if player.is_some() {
            println!("Audio Estereo  : 44.100 Hz Ativo (WinMM)");
        }
        if win_platform::poll_xbox_controller().is_some() {
            println!("Controle Xbox  : Conectado e Ativo (XInput)!");
        } else {
            println!("Controle Xbox  : Aguardando conexao (plug & play ativo)");
        }
        player
    };

    // Janela Arcade Widescreen 16:9 em 512x288 (escala 2x para 1024x576 nitido)
    let mut window = Window::new(
        "PEngine PS1 - PlayStation 1 Arcade",
        hub::CANVAS_WIDTH,
        hub::CANVAS_HEIGHT,
        WindowOptions {
            resize: true,
            scale: minifb::Scale::X2,
            ..WindowOptions::default()
        },
    )
    .expect("Erro ao criar janela grafica!");
    window.set_target_fps(60);

    let is_dev_mode = env::args().any(|a| a == "--dev" || a == "-d");
    let mut monitor = PerformanceMonitor::new(is_dev_mode);

    let mut canvas = vec![0u32; hub::CANVAS_WIDTH * hub::CANVAS_HEIGHT];
    let mut display_buffer = [0u32; TV_WIDTH * TV_HEIGHT];

    let mut in_game_menu = false;
    let mut in_game_tab = 0usize;
    let mut in_game_cursor = 0usize;
    let mut start_hold_frames = 0usize;

    let mut frame_count = 0u64;
    let mut fps_timer = std::time::Instant::now();
    let mut fps_frames = 0u32;


    let mut last_joy_buttons = 0u16;
    let mut game_start_instant: Option<std::time::Instant> = None;
    let mut settings = hub::EmulatorSettings::load();
    let mut display_mode = settings.display_mode;
    let mut aspect_mode = settings.aspect_mode;
    let mut save_slot: usize = settings.save_slot;
    let mut audio_vol: u32 = settings.audio_vol;
    let mut in_welcome = !settings.welcome_completed;
    bus.gpu.config.widescreen_hack = aspect_mode == hub::AspectMode::WidescreenHack;
    let mut cpu_clock_mode: u8 = 0;
    let mut rumble_strength: u8 = 0;

    let mut last_stick_left = false;
    let mut last_stick_right = false;
    let mut last_stick_up = false;
    let mut last_stick_down = false;

    while window.is_open() {
        let gp_opt = {
            #[cfg(target_os = "windows")]
            { win_platform::poll_xbox_controller() }
            #[cfg(not(target_os = "windows"))]
            { None }
        };

        let gamepad_connected = gp_opt.is_some();
        let joy_buttons = gp_opt.as_ref().map(|g| g.w_buttons).unwrap_or(0);
        let joy_just_pressed = joy_buttons & !last_joy_buttons;
        last_joy_buttons = joy_buttons;

        let stick_lx = gp_opt.as_ref().map(|g| g.s_thumb_lx).unwrap_or(0);
        let stick_ly = gp_opt.as_ref().map(|g| g.s_thumb_ly).unwrap_or(0);

        let stick_left = stick_lx < -16000;
        let stick_right = stick_lx > 16000;
        let stick_up = stick_ly > 16000;
        let stick_down = stick_ly < -16000;

        let stick_just_left = stick_left && !last_stick_left;
        let stick_just_right = stick_right && !last_stick_right;
        let stick_just_up = stick_up && !last_stick_up;
        let stick_just_down = stick_down && !last_stick_down;

        last_stick_left = stick_left;
        last_stick_right = stick_right;
        last_stick_up = stick_up;
        last_stick_down = stick_down;

        // Atalho global F11 ou Alt+Enter para tela cheia
        let alt_enter = (window.is_key_down(Key::LeftAlt) || window.is_key_down(Key::RightAlt))
            && window.is_key_pressed(Key::Enter, minifb::KeyRepeat::No);
        if window.is_key_pressed(Key::F12, minifb::KeyRepeat::No) {
            monitor.toggle();
        }
        if window.is_key_pressed(Key::F12, minifb::KeyRepeat::No) {
            monitor.toggle();
        }
        if window.is_key_pressed(Key::F11, minifb::KeyRepeat::No) || alt_enter {
            display_mode = match display_mode {
                hub::DisplayMode::Windowed => hub::DisplayMode::Fullscreen,
                _ => hub::DisplayMode::Windowed,
            };
            apply_window_mode(&window, display_mode);
        }

        match app_mode {
            AppMode::Hub => {
                let left = window.is_key_pressed(Key::Left, minifb::KeyRepeat::No)
                    || (joy_just_pressed & 0x0004) != 0 || stick_just_left;
                let right = window.is_key_pressed(Key::Right, minifb::KeyRepeat::No)
                    || (joy_just_pressed & 0x0008) != 0 || stick_just_right;
                let up = window.is_key_pressed(Key::Up, minifb::KeyRepeat::No)
                    || (joy_just_pressed & 0x0001) != 0 || stick_just_up;
                let down = window.is_key_pressed(Key::Down, minifb::KeyRepeat::No)
                    || (joy_just_pressed & 0x0002) != 0 || stick_just_down;

                let confirm = window.is_key_pressed(Key::Enter, minifb::KeyRepeat::No)
                    || (joy_just_pressed & 0x1000) != 0; // A / Enter
                let cancel = window.is_key_pressed(Key::Escape, minifb::KeyRepeat::No)
                    || (joy_just_pressed & 0x2000) != 0; // B / Esc
                let search_btn = window.is_key_pressed(Key::F2, minifb::KeyRepeat::No)
                    || window.is_key_pressed(Key::X, minifb::KeyRepeat::No)
                    || (joy_just_pressed & 0x4000) != 0; // X / F2 (Busca)
                let open_folder = window.is_key_pressed(Key::F1, minifb::KeyRepeat::No)
                    || window.is_key_pressed(Key::Y, minifb::KeyRepeat::No)
                    || (joy_just_pressed & 0x8000) != 0; // Y / F1 (Pasta ISOs Documentos)
                let fav_btn = window.is_key_pressed(Key::F, minifb::KeyRepeat::No)
                    || (joy_just_pressed & 0x0020) != 0; // Select / F (Favoritos)
                let settings_btn = window.is_key_pressed(Key::F3, minifb::KeyRepeat::No)
                    || window.is_key_pressed(Key::M, minifb::KeyRepeat::No)
                    || (joy_just_pressed & 0x0010) != 0; // Start / F3 (Menu Configuracoes)
                let cat_btn = window.is_key_pressed(Key::Tab, minifb::KeyRepeat::No)
                    || (gp_opt.as_ref().map(|g| g.b_left_trigger > 30).unwrap_or(false) && (joy_just_pressed & 0x0100) != 0); // LT / Tab
                let page_next = window.is_key_pressed(Key::PageDown, minifb::KeyRepeat::No)
                    || (joy_just_pressed & 0x0200) != 0; // RB / PageDown
                let page_prev = window.is_key_pressed(Key::PageUp, minifb::KeyRepeat::No)
                    || (joy_just_pressed & 0x0100) != 0; // LB / PageUp
                let detail_btn = window.is_key_pressed(Key::D, minifb::KeyRepeat::No); // D (Detalhes do Jogo)

                if in_welcome {
                    if up {
                        settings.language = settings.language.prev();
                    }
                    if down {
                        settings.language = settings.language.next();
                    }
                    if confirm {
                        settings.welcome_completed = true;
                        settings.save();
                        in_welcome = false;
                    }

                    hub::render_welcome_screen(&mut canvas, settings.language, gamepad_connected);
                    window
                        .update_with_buffer(&canvas, hub::CANVAS_WIDTH, hub::CANVAS_HEIGHT)
                        .expect("Erro ao atualizar tela de boas-vindas!");
                    continue;
                }

                if hub_state.in_settings_menu {
                    // Modal de Configurações Avançadas (START no Hub)
                    let lb = page_prev || window.is_key_pressed(Key::Key1, minifb::KeyRepeat::No);
                    let rb = page_next || window.is_key_pressed(Key::Key2, minifb::KeyRepeat::No);
                    if lb {
                        hub_state.settings_tab = if hub_state.settings_tab == 0 { 3 } else { hub_state.settings_tab - 1 };
                        hub_state.settings_row = 0;
                    }
                    if rb {
                        hub_state.settings_tab = (hub_state.settings_tab + 1) % 4;
                        hub_state.settings_row = 0;
                    }

                    let max_rows = match hub_state.settings_tab {
                        0 => 7,
                        1 => 4,
                        2 => 5,
                        3 => 4,
                        _ => 1,
                    };
                    if up && hub_state.settings_row > 0 { hub_state.settings_row -= 1; }
                    if down && hub_state.settings_row + 1 < max_rows { hub_state.settings_row += 1; }

                    if left || right || confirm {
                        match hub_state.settings_tab {
                            0 => match hub_state.settings_row {
                                0 => {
                                    display_mode = if left { display_mode.prev() } else { display_mode.next() };
                                    apply_window_mode(&window, display_mode);
                                }
                                1 => {
                                    aspect_mode = if left { aspect_mode.prev() } else { aspect_mode.next() };
                                    bus.gpu.config.widescreen_hack = aspect_mode == hub::AspectMode::WidescreenHack;
                                }
                                2 => {
                                    bus.gpu.config.internal_res = match bus.gpu.config.internal_res {
                                        gpu::InternalResolution::Native1x => gpu::InternalResolution::Hd2x,
                                        gpu::InternalResolution::Hd2x => gpu::InternalResolution::FullHd4x,
                                        gpu::InternalResolution::FullHd4x => gpu::InternalResolution::Native1x,
                                    };
                                }
                                3 => {
                                    bus.gpu.config.pgxp_enabled = !bus.gpu.config.pgxp_enabled;
                                }
                                4 => {
                                    bus.gpu.config.perspective_correct = !bus.gpu.config.perspective_correct;
                                }
                                5 => {
                                    bus.gpu.config.bilinear_filter = !bus.gpu.config.bilinear_filter;
                                }
                                6 => {
                                    bus.gpu.config.shader = match bus.gpu.config.shader {
                                        gpu::ShaderEffect::None => gpu::ShaderEffect::Bilinear,
                                        gpu::ShaderEffect::Bilinear => gpu::ShaderEffect::CrtScanlines,
                                        gpu::ShaderEffect::CrtScanlines => gpu::ShaderEffect::BilinearCrt,
                                        gpu::ShaderEffect::BilinearCrt => gpu::ShaderEffect::RfNoise,
                                        gpu::ShaderEffect::RfNoise => gpu::ShaderEffect::None,
                                    };
                                }
                                _ => {}
                            },
                            1 => match hub_state.settings_row {
                                0 => {
                                    audio_vol = if left {
                                        if audio_vol >= 20 { audio_vol - 20 } else { 100 }
                                    } else {
                                        if audio_vol <= 80 { audio_vol + 20 } else { 0 }
                                    };
                                }
                                1 => {}
                                2 => {}
                                3 => {}
                                _ => {}
                            },
                            2 => match hub_state.settings_row {
                                0 => {
                                    bus.gpu.config.render_mode = match bus.gpu.config.render_mode {
                                        gpu::RenderMode::Hardware => gpu::RenderMode::Software,
                                        gpu::RenderMode::Software => gpu::RenderMode::Hardware,
                                    };
                                }
                                1 => cpu_clock_mode = (cpu_clock_mode + 1) % 3,
                                2 => save_slot = (save_slot + 1) % 5,
                                3 => {
                                    let isos_dir = hub::get_documents_isos_dir();
                                    let _ = std::process::Command::new("explorer").arg(&isos_dir).spawn();
                                }
                                4 => {
                                    let bios_dir = hub::get_documents_bios_dir();
                                    let _ = std::process::Command::new("explorer").arg(&bios_dir).spawn();
                                }
                                _ => {}
                            },
                            3 => match hub_state.settings_row {
                                0 => {}
                                1 => bus.controller.is_analog = !bus.controller.is_analog,
                                2 => rumble_strength = (rumble_strength + 1) % 3,
                                3 => {}
                                _ => {}
                            },
                            _ => {}
                        }
                    }

                    if cancel {
                        hub_state.in_settings_menu = false;
                        settings.display_mode = display_mode;
                        settings.aspect_mode = aspect_mode;
                        settings.save_slot = save_slot;
                        settings.audio_vol = audio_vol;
                        settings.save();
                    }

                    hub::render_advanced_settings_menu(
                        &mut canvas,
                        hub_state.settings_tab,
                        hub_state.settings_row,
                        gamepad_connected,
                        &bus.gpu.config,
                        display_mode,
                        aspect_mode,
                        save_slot,
                        audio_vol,
                        cpu_clock_mode,
                        rumble_strength,
                    );
                    window.update_with_buffer(&canvas, hub::CANVAS_WIDTH, hub::CANVAS_HEIGHT).expect("Erro ao atualizar tela!");
                                        continue;
                }

                if hub_state.in_detail_card {
                    // Modal de Detalhes do Jogo
                    if left && hub_state.detail_cursor > 0 { hub_state.detail_cursor -= 1; }
                    if right && hub_state.detail_cursor < 2 { hub_state.detail_cursor += 1; }

                    if confirm {
                        match hub_state.detail_cursor {
                            0 => {
                                // JOGAR
                                if !hub_state.filtered_indices.is_empty() {
                                    let sel_idx = hub_state.filtered_indices[hub_state.selected_pos];
                                    let game = &mut games[sel_idx];
                                    game.stats.play_count += 1;
                                    let path_to_boot = game.path.clone();
                                    let title_to_show = game.clean_title.clone();
                                    hub::save_game_stats(&game.raw_name, &game.stats);

                                    if boot_game(&path_to_boot, &bios_bytes, &mut cpu, &mut bus) {
                                        current_game_path = path_to_boot;
                                        current_game_title = title_to_show;
                                        app_mode = AppMode::InGame;
                                        game_start_instant = Some(std::time::Instant::now());
                                        hub_state.in_detail_card = false;
                                    }
                                }
                            }
                            1 => {
                                // RESETAR ESTATISTICAS
                                if !hub_state.filtered_indices.is_empty() {
                                    let sel_idx = hub_state.filtered_indices[hub_state.selected_pos];
                                    games[sel_idx].stats.play_count = 0;
                                    games[sel_idx].stats.play_time_seconds = 0;
                                    hub::save_game_stats(&games[sel_idx].raw_name, &games[sel_idx].stats);
                                }
                            }
                            2 => {
                                // VOLTAR
                                hub_state.in_detail_card = false;
                            }
                            _ => {}
                        }
                    } else if cancel {
                        hub_state.in_detail_card = false;
                    }

                    if !hub_state.filtered_indices.is_empty() {
                        let sel_idx = hub_state.filtered_indices[hub_state.selected_pos];
                        hub::render_game_detail_card(&mut canvas, &games[sel_idx], gamepad_connected, hub_state.detail_cursor);
                    }
                    window.update_with_buffer(&canvas, hub::CANVAS_WIDTH, hub::CANVAS_HEIGHT).expect("Erro ao atualizar tela!");
                                        continue;
                }

                if hub_state.in_search_mode {
                    // Modal de Teclado Virtual
                    if left && hub_state.vkb_cursor > 0 { hub_state.vkb_cursor -= 1; }
                    if right && hub_state.vkb_cursor + 1 < hub::VIRTUAL_KB_CHARS.len() { hub_state.vkb_cursor += 1; }
                    if up && hub_state.vkb_cursor >= 10 { hub_state.vkb_cursor -= 10; }
                    if down && hub_state.vkb_cursor + 10 < hub::VIRTUAL_KB_CHARS.len() { hub_state.vkb_cursor += 10; }

                    if confirm {
                        let selected_k = hub::VIRTUAL_KB_CHARS[hub_state.vkb_cursor];
                        match selected_k {
                            "OK" => hub_state.in_search_mode = false,
                            "ESPACO" => { hub_state.search_query.push(' '); hub_state.rebuild_filter(&games); }
                            "APAGAR" => { hub_state.search_query.pop(); hub_state.rebuild_filter(&games); }
                            "LIMPAR" => { hub_state.search_query.clear(); hub_state.rebuild_filter(&games); }
                            k => { hub_state.search_query.push_str(k); hub_state.rebuild_filter(&games); }
                        }
                    } else if cancel {
                        hub_state.in_search_mode = false;
                    }
                } else {
                    // Carrossel Normal da Biblioteca Arcade
                    if settings_btn {
                        hub_state.in_settings_menu = true;
                        hub_state.settings_tab = 0;
                        hub_state.settings_row = 0;
                    }
                    if open_folder {
                        let isos_dir = hub::get_documents_isos_dir();
                        let _ = std::process::Command::new("explorer").arg(&isos_dir).spawn();
                        games = hub::scan_games();
                        hub_state.rebuild_filter(&games);
                    }
                    if detail_btn && !hub_state.filtered_indices.is_empty() {
                        hub_state.in_detail_card = true;
                        hub_state.detail_cursor = 0;
                    }
                    if cat_btn {
                        hub_state.category = hub_state.category.next();
                        hub_state.rebuild_filter(&games);
                    }
                    if search_btn {
                        hub_state.in_search_mode = true;
                    }
                    if fav_btn && !hub_state.filtered_indices.is_empty() {
                        let idx = hub_state.filtered_indices[hub_state.selected_pos];
                        games[idx].stats.is_favorite = !games[idx].stats.is_favorite;
                        hub::save_game_stats(&games[idx].raw_name, &games[idx].stats);
                    }

                    if page_next && !hub_state.filtered_indices.is_empty() {
                        let cur_c = games[hub_state.filtered_indices[hub_state.selected_pos]].first_char;
                        if let Some(pos) = hub_state.filtered_indices.iter().position(|&i| games[i].first_char > cur_c) {
                            hub_state.selected_pos = pos;
                        }
                    }
                    if page_prev && !hub_state.filtered_indices.is_empty() {
                        let cur_c = games[hub_state.filtered_indices[hub_state.selected_pos]].first_char;
                        if let Some(pos) = hub_state.filtered_indices.iter().rposition(|&i| games[i].first_char < cur_c) {
                            hub_state.selected_pos = pos;
                        }
                    }

                    if left && !hub_state.filtered_indices.is_empty() {
                        if hub_state.selected_pos > 0 {
                            hub_state.selected_pos -= 1;
                        } else {
                            hub_state.selected_pos = hub_state.filtered_indices.len() - 1;
                        }
                    }
                    if right && !hub_state.filtered_indices.is_empty() {
                        if hub_state.selected_pos + 1 < hub_state.filtered_indices.len() {
                            hub_state.selected_pos += 1;
                        } else {
                            hub_state.selected_pos = 0;
                        }
                    }

                    if confirm && !hub_state.filtered_indices.is_empty() {
                        let sel_idx = hub_state.filtered_indices[hub_state.selected_pos];
                        let game = &mut games[sel_idx];
                        game.stats.play_count += 1;
                        hub::save_game_stats(&game.raw_name, &game.stats);

                        let path_to_boot = game.path.clone();
                        let title_to_show = game.clean_title.clone();

                        if boot_game(&path_to_boot, &bios_bytes, &mut cpu, &mut bus) {
                            current_game_path = path_to_boot;
                            current_game_title = title_to_show;
                            app_mode = AppMode::InGame;
                            game_start_instant = Some(std::time::Instant::now());
                            in_game_menu = false;
                        }
                    } else if cancel {
                        break;
                    }
                }

                let mut visible_indices = Vec::new();
                if !hub_state.filtered_indices.is_empty() {
                    let sel = hub_state.selected_pos;
                    let total = hub_state.filtered_indices.len();
                    for offset in [-2isize, -1, 0, 1, 2] {
                        let idx = (sel as isize + offset).rem_euclid(total as isize) as usize;
                        visible_indices.push(hub_state.filtered_indices[idx]);
                    }
                }
                cover_loader.poll_and_request(&mut games, &visible_indices);

                hub::render_arcade_hub_frame(&mut canvas, &games, &hub_state, gamepad_connected);
                window.update_with_buffer(&canvas, hub::CANVAS_WIDTH, hub::CANVAS_HEIGHT).expect("Erro ao atualizar tela!");
                            }

            AppMode::InGame => {
                // DETECÇÃO DE SEGURAR O BOTÃO START (~45 FRAMES = ~0.75s) SOMENTE NO JOGO
                let start_is_held = (joy_buttons & 0x0010) != 0 || window.is_key_down(Key::Space);
                if start_is_held {
                    start_hold_frames += 1;
                } else {
                    start_hold_frames = 0;
                }

                let hold_triggered = start_hold_frames == 45;
                let esc_pressed = window.is_key_pressed(Key::Escape, minifb::KeyRepeat::No);
                let f3_pressed = window.is_key_pressed(Key::F3, minifb::KeyRepeat::No);

                if hold_triggered || esc_pressed || f3_pressed {
                    in_game_menu = !in_game_menu;
                    in_game_tab = 0;
                    in_game_cursor = 0;
                    start_hold_frames = 0;
                }

                if in_game_menu {
                    let page_prev = (joy_just_pressed & 0x0100) != 0 || window.is_key_pressed(Key::Key1, minifb::KeyRepeat::No);
                    let page_next = (joy_just_pressed & 0x0200) != 0 || window.is_key_pressed(Key::Key2, minifb::KeyRepeat::No);

                    if page_prev {
                        in_game_tab = if in_game_tab == 0 { 2 } else { in_game_tab - 1 };
                        in_game_cursor = 0;
                    }
                    if page_next {
                        in_game_tab = (in_game_tab + 1) % 3;
                        in_game_cursor = 0;
                    }

                    let up = window.is_key_pressed(Key::Up, minifb::KeyRepeat::No) || (joy_just_pressed & 0x0001) != 0;
                    let down = window.is_key_pressed(Key::Down, minifb::KeyRepeat::No) || (joy_just_pressed & 0x0002) != 0;
                    let left = window.is_key_pressed(Key::Left, minifb::KeyRepeat::No) || (joy_just_pressed & 0x0004) != 0;
                    let right = window.is_key_pressed(Key::Right, minifb::KeyRepeat::No) || (joy_just_pressed & 0x0008) != 0;
                    let confirm = window.is_key_pressed(Key::Enter, minifb::KeyRepeat::No) || (joy_just_pressed & 0x1000) != 0;
                    let cancel = (joy_just_pressed & 0x2000) != 0;

                    let max_cursor = match in_game_tab { 0 => 5, 1 => 5, _ => 0 };
                    if up && in_game_cursor > 0 { in_game_cursor -= 1; }
                    if down && in_game_cursor < max_cursor { in_game_cursor += 1; }

                    if cancel {
                        in_game_menu = false;
                        start_hold_frames = 0;
                    } else if confirm || left || right {
                        match in_game_tab {
                            0 => {
                                // ABA 1. PARTIDA
                                match in_game_cursor {
                                    0 => in_game_menu = false, // Continuar
                                    1 => {
                                        // Salvar Estado
                                        let save_file = format!("{}_slot{}.state", current_game_title, save_slot + 1);
                                        let _ = savestate::save_state(&save_file, &cpu, &bus);
                                        println!(">>> Estado salvo no Slot {}!", save_slot + 1);
                                    }
                                    2 => {
                                        // Carregar Estado
                                        let save_file = format!("{}_slot{}.state", current_game_title, save_slot + 1);
                                        if savestate::load_state(&save_file, &mut cpu, &mut bus).is_ok() {
                                            println!(">>> Estado carregado do Slot {}!", save_slot + 1);
                                            in_game_menu = false;
                                        }
                                    }
                                    3 => save_slot = (save_slot + 1) % 5,
                                    4 => {
                                        // Reiniciar console
                                        boot_game(&current_game_path, &bios_bytes, &mut cpu, &mut bus);
                                        in_game_menu = false;
                                    }
                                    5 => {
                                        // Sair para o Hub
                                        if let Some(st) = game_start_instant {
                                            let played_secs = st.elapsed().as_secs();
                                            if let Some(pos) = games.iter().position(|g| g.clean_title == current_game_title) {
                                                games[pos].stats.play_time_seconds += played_secs;
                                                hub::save_game_stats(&games[pos].raw_name, &games[pos].stats);
                                            }
                                        }
                                        app_mode = AppMode::Hub;
                                        in_game_menu = false;
                                        games = hub::scan_games();
                                        hub_state.rebuild_filter(&games);
                                    }
                                    _ => {}
                                }
                            }
                            1 => {
                                // ABA 2. VIDEO (IN-GAME)
                                match in_game_cursor {
                                    0 => {
                                        display_mode = if left { display_mode.prev() } else { display_mode.next() };
                                        apply_window_mode(&window, display_mode);
                                    }
                                    1 => {
                                        aspect_mode = if left { aspect_mode.prev() } else { aspect_mode.next() };
                                        bus.gpu.config.widescreen_hack = aspect_mode == hub::AspectMode::WidescreenHack;
                                    }
                                    2 => {
                                        bus.gpu.config.internal_res = match bus.gpu.config.internal_res {
                                            gpu::InternalResolution::Native1x => gpu::InternalResolution::Hd2x,
                                            gpu::InternalResolution::Hd2x => gpu::InternalResolution::FullHd4x,
                                            gpu::InternalResolution::FullHd4x => gpu::InternalResolution::Native1x,
                                        };
                                    }
                                    3 => bus.gpu.config.pgxp_enabled = !bus.gpu.config.pgxp_enabled,
                                    4 => bus.gpu.config.bilinear_filter = !bus.gpu.config.bilinear_filter,
                                    5 => {
                                        bus.gpu.config.shader = match bus.gpu.config.shader {
                                            gpu::ShaderEffect::None => gpu::ShaderEffect::Bilinear,
                                            gpu::ShaderEffect::Bilinear => gpu::ShaderEffect::CrtScanlines,
                                            gpu::ShaderEffect::CrtScanlines => gpu::ShaderEffect::BilinearCrt,
                                            gpu::ShaderEffect::BilinearCrt => gpu::ShaderEffect::RfNoise,
                                            gpu::ShaderEffect::RfNoise => gpu::ShaderEffect::None,
                                        };
                                    }
                                    _ => {}
                                }
                            }
                            2 => {
                                // ABA 3. AUDIO (IN-GAME)
                                if in_game_cursor == 0 {
                                    audio_vol = if left {
                                        if audio_vol >= 20 { audio_vol - 20 } else { 100 }
                                    } else {
                                        if audio_vol <= 80 { audio_vol + 20 } else { 0 }
                                    };
                                }
                            }
                            _ => {}
                        }
                    }

                    let state_file = format!("{}_slot{}.state", current_game_title, save_slot + 1);
                    let has_save = Path::new(&state_file).exists();

                    hub::render_in_game_menu(
                        &mut canvas,
                        &current_game_title,
                        has_save,
                        in_game_tab,
                        in_game_cursor,
                        gamepad_connected,
                        &bus.gpu.config,
                        display_mode,
                        aspect_mode,
                        save_slot,
                        audio_vol,
                    );
                    window.update_with_buffer(&canvas, hub::CANVAS_WIDTH, hub::CANVAS_HEIGHT).expect("Erro ao atualizar tela!");
                                        continue;
                }

                // Emulação Ativa a 60 FPS
                update_controller_inputs(&window, &mut bus.controller);

                let emu_start = std::time::Instant::now();
                run_one_frame(&mut cpu, &mut bus, match cpu_clock_mode { 1 => 1.5, 2 => 2.0, _ => 1.0 });
                let emu_duration = emu_start.elapsed();

                #[cfg(target_os = "windows")]
                if let Some(player) = &mut audio_player {
                    if audio_vol > 0 {
                        let vol_pct = audio_vol as i32;
                        let scaled: Vec<i16> = bus.spu.audio_out.iter().map(|&s| {
                            ((s as i32 * vol_pct) / 100).clamp(-32768, 32767) as i16
                        }).collect();
                        player.submit_samples(&scaled);
                    }
                }
                bus.spu.audio_out.clear();

                #[cfg(target_os = "windows")]
                {
                    let (l_rumble, r_rumble) = bus.controller.get_rumble();
                    win_platform::set_xbox_rumble(l_rumble, r_rumble);
                }

                // Renderiza GPU nativa PS1
                bus.gpu.render_to_display_buffer(&mut display_buffer);

                // Blita o jogo no Canvas 16:9 512x288 do Arcade Hub
                blit_ps1_to_canvas(&display_buffer, &mut canvas, aspect_mode);

                window
                    .update_with_buffer(&canvas, hub::CANVAS_WIDTH, hub::CANVAS_HEIGHT)
                    .expect("Erro ao atualizar tela do jogo!");

                if monitor.is_open() && (frame_count % 6 == 0) {
                    let current_fps = (fps_frames as f32) / fps_timer.elapsed().as_secs_f32().max(0.001);
                    monitor.update(&cpu, &bus, current_fps, emu_duration);
                }

                frame_count += 1;
                fps_frames += 1;

                let elapsed = fps_timer.elapsed().as_secs_f32();
                if elapsed >= 0.5 {
                    let real_fps = (fps_frames as f32) / elapsed;
                    let pad_mode = if bus.controller.is_analog { "DualShock" } else { "Digital" };
                    let render_mode_str = match bus.gpu.config.render_mode {
                        gpu::RenderMode::Hardware => {
                            if bus.gpu.config.pgxp_enabled {
                                "Hardware HD (PGXP)"
                            } else {
                                "Hardware HD"
                            }
                        }
                        gpu::RenderMode::Software => "Software (PS1)",
                    };
                    let shader_str = match bus.gpu.config.shader {
                        gpu::ShaderEffect::None => "PixelArt",
                        gpu::ShaderEffect::Bilinear => "Bilinear",
                        gpu::ShaderEffect::CrtScanlines => "CRT",
                        gpu::ShaderEffect::BilinearCrt => "Bilinear+CRT",
                        gpu::ShaderEffect::RfNoise => "RF Retro",
                    };

                    window.set_title(&format!(
                        "PEngine PSX - {:.1} FPS | [{}] | [{}] | [{}] | {}",
                        real_fps, pad_mode, render_mode_str, shader_str, current_game_title
                    ));
                    fps_timer = std::time::Instant::now();
                    fps_frames = 0;
                }

                // Frame Pacing fluido sincronizado pelo minifb em 60 FPS exatos
            }
        }
    }
}

/// Atualiza os botões do PS1 somando Teclado + Controle de Xbox (D-Pad e Analógico Esquerdo)
fn update_controller_inputs(window: &Window, pad: &mut controller::Controller) {
    if window.is_key_pressed(Key::F5, minifb::KeyRepeat::No) {
        pad.is_analog = !pad.is_analog;
    }
    let mut up = window.is_key_down(Key::Up);
    let mut down = window.is_key_down(Key::Down);
    let mut left = window.is_key_down(Key::Left);
    let mut right = window.is_key_down(Key::Right);

    let mut cross = window.is_key_down(Key::Z);
    let mut circle = window.is_key_down(Key::X);
    let mut square = window.is_key_down(Key::A);
    let mut triangle = window.is_key_down(Key::S);

    let mut start = window.is_key_down(Key::Enter);
    let mut select = window.is_key_down(Key::Space);

    let mut l1 = window.is_key_down(Key::Q);
    let mut r1 = window.is_key_down(Key::E);
    let mut l2 = window.is_key_down(Key::Key1);
    let mut r2 = window.is_key_down(Key::Key3);

    #[cfg(target_os = "windows")]
    if let Some(gp) = win_platform::poll_xbox_controller() {
        let b = gp.w_buttons;
        const DEADZONE: i16 = 8000;

        up |= (b & 0x0001) != 0 || gp.s_thumb_ly > DEADZONE;
        down |= (b & 0x0002) != 0 || gp.s_thumb_ly < -DEADZONE;
        left |= (b & 0x0004) != 0 || gp.s_thumb_lx < -DEADZONE;
        right |= (b & 0x0008) != 0 || gp.s_thumb_lx > DEADZONE;

        start |= (b & 0x0010) != 0;
        select |= (b & 0x0020) != 0;

        l1 |= (b & 0x0100) != 0;
        r1 |= (b & 0x0200) != 0;
        l2 |= gp.b_left_trigger > 30;
        r2 |= gp.b_right_trigger > 30;

        cross |= (b & 0x1000) != 0;
        circle |= (b & 0x2000) != 0;
        square |= (b & 0x4000) != 0;
        triangle |= (b & 0x8000) != 0;

        pad.left_x = ((gp.s_thumb_lx as i32 + 32768) >> 8).clamp(0, 255) as u8;
        pad.left_y = ((32767 - gp.s_thumb_ly as i32) >> 8).clamp(0, 255) as u8;
        pad.right_x = ((gp.s_thumb_rx as i32 + 32768) >> 8).clamp(0, 255) as u8;
        pad.right_y = ((32767 - gp.s_thumb_ry as i32) >> 8).clamp(0, 255) as u8;
    }

    let apply = |pressed: bool, btn: Button, pad: &mut controller::Controller| {
        if pressed {
            pad.press_button(btn);
        } else {
            pad.release_button(btn);
        }
    };

    apply(up, Button::Up, pad);
    apply(down, Button::Down, pad);
    apply(left, Button::Left, pad);
    apply(right, Button::Right, pad);
    apply(cross, Button::Cross, pad);
    apply(circle, Button::Circle, pad);
    apply(square, Button::Square, pad);
    apply(triangle, Button::Triangle, pad);
    apply(start, Button::Start, pad);
    apply(select, Button::Select, pad);
    apply(l1, Button::L1, pad);
    apply(r1, Button::R1, pad);
    apply(l2, Button::L2, pad);
    apply(r2, Button::R2, pad);
}
