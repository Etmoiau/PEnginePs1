// src/hub.rs
// Sistema de Hub Arcade Moderno (Biblioteca Visual, Carrossel 3D, Busca, Favoritos e Menus por Abas)
// Projetado para o emulador PS1 (meu_psx) com a mesma interface gráfica do PEngine.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

#[cfg(not(target_os = "windows"))]
use std::time::SystemTime;


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayMode {
    Windowed = 0,
    Borderless = 1,
    Fullscreen = 2,
}

impl DisplayMode {
    pub fn next(self) -> Self {
        match self {
            Self::Windowed => Self::Borderless,
            Self::Borderless => Self::Fullscreen,
            Self::Fullscreen => Self::Windowed,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Self::Windowed => Self::Fullscreen,
            Self::Borderless => Self::Windowed,
            Self::Fullscreen => Self::Borderless,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AspectMode {
    Classic4x3 = 0,
    WidescreenHack = 1,
    WidescreenStretch = 2,
}

impl AspectMode {
    pub fn next(self) -> Self {
        match self {
            Self::Classic4x3 => Self::WidescreenHack,
            Self::WidescreenHack => Self::WidescreenStretch,
            Self::WidescreenStretch => Self::Classic4x3,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Self::Classic4x3 => Self::WidescreenStretch,
            Self::WidescreenHack => Self::Classic4x3,
            Self::WidescreenStretch => Self::WidescreenHack,
        }
    }
}

pub fn is_system_or_emulator_file(stem: &str, ext: &str, file_size: u64) -> bool {
    let lower_stem = stem.to_lowercase();
    let lower_ext = ext.to_lowercase();

    // 1. Arquivos de BIOS
    if lower_stem.starts_with("scph") || lower_stem.starts_with("ps-") || lower_stem == "bios" || lower_stem.starts_with("dtlh") {
        return true;
    }
    if lower_ext == "bin" && file_size == 524288 {
        return true;
    }

    // 2. Executáveis do próprio emulador e ferramentas
    if lower_stem.contains("meu_psx") || lower_stem.contains("pengine") || lower_stem.contains("cargo") 
        || lower_stem.contains("rust") || lower_stem.contains("target") || lower_stem == "main" {
        return true;
    }

    // 3. Faixas secundárias de áudio (Track 2, Track 3, etc.)
    if lower_stem.contains("track") {
        let is_track1 = lower_stem.contains("track 1") || lower_stem.contains("track 01") || lower_stem.contains("track_1") || lower_stem.contains("track_01");
        if !is_track1 {
            return true;
        }
    }

    false
}

pub const CANVAS_WIDTH: usize = 512;
pub const CANVAS_HEIGHT: usize = 288;
pub const CARD_WIDTH: usize = 90;
pub const CARD_HEIGHT: usize = 130;

// ============================================================================
// FONTE BITMAP 5x7 COMPACTA E ZERO-ALLOC
// ============================================================================
const FONT_5X7: [[u8; 5]; 96] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], [0x00, 0x00, 0x5F, 0x00, 0x00], [0x00, 0x07, 0x00, 0x07, 0x00],
    [0x14, 0x7F, 0x14, 0x7F, 0x14], [0x24, 0x2A, 0x7F, 0x2A, 0x12], [0x23, 0x13, 0x08, 0x64, 0x62],
    [0x36, 0x49, 0x55, 0x22, 0x50], [0x00, 0x05, 0x03, 0x00, 0x00], [0x00, 0x1C, 0x22, 0x41, 0x00],
    [0x00, 0x41, 0x22, 0x1C, 0x00], [0x14, 0x08, 0x3E, 0x08, 0x14], [0x08, 0x08, 0x3E, 0x08, 0x08],
    [0x00, 0x50, 0x30, 0x00, 0x00], [0x08, 0x08, 0x08, 0x08, 0x08], [0x00, 0x60, 0x60, 0x00, 0x00],
    [0x20, 0x10, 0x08, 0x04, 0x02], [0x3E, 0x51, 0x49, 0x45, 0x3E], [0x00, 0x42, 0x7F, 0x40, 0x00],
    [0x42, 0x61, 0x51, 0x49, 0x46], [0x21, 0x41, 0x45, 0x4B, 0x31], [0x18, 0x14, 0x12, 0x7F, 0x10],
    [0x27, 0x45, 0x45, 0x45, 0x39], [0x3C, 0x4A, 0x49, 0x49, 0x30], [0x01, 0x71, 0x09, 0x05, 0x03],
    [0x36, 0x49, 0x49, 0x49, 0x36], [0x06, 0x49, 0x49, 0x29, 0x1E], [0x00, 0x36, 0x36, 0x00, 0x00],
    [0x00, 0x56, 0x36, 0x00, 0x00], [0x08, 0x14, 0x22, 0x41, 0x00], [0x14, 0x14, 0x14, 0x14, 0x14],
    [0x00, 0x41, 0x22, 0x14, 0x08], [0x02, 0x01, 0x51, 0x09, 0x06], [0x32, 0x49, 0x79, 0x41, 0x3E],
    [0x7E, 0x11, 0x11, 0x11, 0x7E], [0x7F, 0x49, 0x49, 0x49, 0x36], [0x3E, 0x41, 0x41, 0x41, 0x22],
    [0x7F, 0x41, 0x41, 0x22, 0x1C], [0x7F, 0x49, 0x49, 0x49, 0x41], [0x7F, 0x09, 0x09, 0x09, 0x01],
    [0x3E, 0x41, 0x49, 0x49, 0x7A], [0x7F, 0x08, 0x08, 0x08, 0x7F], [0x00, 0x41, 0x7F, 0x41, 0x00],
    [0x20, 0x40, 0x41, 0x3F, 0x01], [0x7F, 0x08, 0x14, 0x22, 0x41], [0x7F, 0x40, 0x40, 0x40, 0x40],
    [0x7F, 0x02, 0x0C, 0x02, 0x7F], [0x7F, 0x04, 0x08, 0x10, 0x7F], [0x3E, 0x41, 0x41, 0x41, 0x3E],
    [0x7F, 0x09, 0x09, 0x09, 0x06], [0x3E, 0x41, 0x51, 0x21, 0x5E], [0x7F, 0x09, 0x19, 0x29, 0x46],
    [0x46, 0x49, 0x49, 0x49, 0x31], [0x01, 0x01, 0x7F, 0x01, 0x01], [0x3F, 0x40, 0x40, 0x40, 0x3F],
    [0x1F, 0x20, 0x40, 0x20, 0x1F], [0x7F, 0x20, 0x18, 0x20, 0x7F], [0x63, 0x14, 0x08, 0x14, 0x63],
    [0x07, 0x08, 0x70, 0x08, 0x07], [0x61, 0x51, 0x49, 0x45, 0x43], [0x00, 0x7F, 0x41, 0x41, 0x00],
    [0x02, 0x04, 0x08, 0x10, 0x20], [0x00, 0x41, 0x41, 0x7F, 0x00], [0x04, 0x02, 0x01, 0x02, 0x04],
    [0x40, 0x40, 0x40, 0x40, 0x40], [0x00, 0x01, 0x02, 0x04, 0x00], [0x20, 0x54, 0x54, 0x54, 0x78],
    [0x7F, 0x48, 0x44, 0x44, 0x38], [0x38, 0x44, 0x44, 0x44, 0x20], [0x38, 0x44, 0x44, 0x48, 0x7F],
    [0x38, 0x54, 0x54, 0x54, 0x18], [0x08, 0x7E, 0x09, 0x01, 0x02], [0x0C, 0x52, 0x52, 0x52, 0x3E],
    [0x7F, 0x08, 0x04, 0x04, 0x78], [0x00, 0x44, 0x7D, 0x40, 0x00], [0x20, 0x40, 0x44, 0x3D, 0x00],
    [0x7F, 0x10, 0x28, 0x44, 0x00], [0x00, 0x41, 0x7F, 0x40, 0x00], [0x7C, 0x04, 0x18, 0x04, 0x78],
    [0x7C, 0x08, 0x04, 0x04, 0x78], [0x38, 0x44, 0x44, 0x44, 0x38], [0x7C, 0x14, 0x14, 0x14, 0x08],
    [0x08, 0x14, 0x14, 0x18, 0x7C], [0x7C, 0x08, 0x04, 0x04, 0x08], [0x48, 0x54, 0x54, 0x54, 0x20],
    [0x04, 0x3F, 0x44, 0x40, 0x20], [0x3C, 0x40, 0x40, 0x20, 0x7C], [0x1C, 0x20, 0x40, 0x20, 0x1C],
    [0x3C, 0x40, 0x30, 0x40, 0x3C], [0x44, 0x28, 0x10, 0x28, 0x44], [0x0C, 0x50, 0x50, 0x50, 0x3C],
    [0x44, 0x64, 0x54, 0x4C, 0x44], [0x00, 0x08, 0x36, 0x41, 0x00], [0x00, 0x00, 0x7F, 0x00, 0x00],
    [0x00, 0x41, 0x36, 0x08, 0x00], [0x08, 0x04, 0x08, 0x10, 0x08], [0x7F, 0x7F, 0x7F, 0x7F, 0x7F],
];

pub fn draw_text(buffer: &mut [u32], width: usize, height: usize, text: &str, x: usize, y: usize, color: u32) {
    let mut cur_x = x;
    for ch in text.chars() {
        let code = ch as usize;
        if (32..127).contains(&code) {
            let glyph = &FONT_5X7[code - 32];
            for (col_idx, &col) in glyph.iter().enumerate() {
                for row_idx in 0..7 {
                    if (col & (1 << row_idx)) != 0 {
                        let px = cur_x + col_idx;
                        let py = y + row_idx;
                        if px < width && py < height {
                            buffer[py * width + px] = color;
                        }
                    }
                }
            }
        }
        cur_x += 6;
    }
}

pub fn draw_text_centered(buffer: &mut [u32], width: usize, height: usize, box_x: usize, box_w: usize, y: usize, text: &str, color: u32) {
    let tw = text.len() * 6;
    let tx = box_x + box_w.saturating_sub(tw) / 2;
    draw_text(buffer, width, height, text, tx, y, color);
}

pub fn draw_rect(buffer: &mut [u32], width: usize, height: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    for row in y..(y + h).min(height) {
        for col in x..(x + w).min(width) {
            buffer[row * width + col] = color;
        }
    }
}

pub fn badge_pixel_width(label: &str) -> usize {
    (label.len() * 6 + 6).max(14)
}

pub fn draw_button_badge(
    buffer: &mut [u32],
    canvas_w: usize,
    canvas_h: usize,
    label: &str,
    x: usize,
    y: usize,
    bg_color: u32,
    text_color: u32,
) {
    let pad_w = badge_pixel_width(label);
    let pad_h = 11;

    for row in 0..pad_h {
        let py = y + row;
        if py >= canvas_h { continue; }
        for col in 0..pad_w {
            let px = x + col;
            if px >= canvas_w { continue; }

            let is_corner = (row == 0 || row == pad_h - 1) && (col == 0 || col == pad_w - 1);
            if !is_corner {
                buffer[py * canvas_w + px] = bg_color;
            }
        }
    }

    let text_w = label.len() * 6;
    let text_x = x + pad_w.saturating_sub(text_w) / 2;
    draw_text(buffer, canvas_w, canvas_h, label, text_x, y + 2, text_color);
}

pub fn draw_centered_badge_row(
    buffer: &mut [u32],
    canvas_w: usize,
    canvas_h: usize,
    area_x: usize,
    area_w: usize,
    y: usize,
    item_gap: usize,
    items: &[(&str, u32, u32, &str)],
) {
    if items.is_empty() {
        return;
    }

    let inner_gap = 4;
    let mut total_w = 0usize;
    for (i, &(badge, _, _, label)) in items.iter().enumerate() {
        total_w += badge_pixel_width(badge) + inner_gap + (label.len() * 6);
        if i + 1 < items.len() {
            total_w += item_gap;
        }
    }

    let mut cur_x = area_x + area_w.saturating_sub(total_w) / 2;
    for &(badge, bg, fg, label) in items {
        let bw = badge_pixel_width(badge);
        draw_button_badge(buffer, canvas_w, canvas_h, badge, cur_x, y, bg, fg);
        cur_x += bw + inner_gap;
        draw_text(buffer, canvas_w, canvas_h, label, cur_x, y + 2, 0x90A4AE);
        cur_x += (label.len() * 6) + item_gap;
    }
}

pub fn get_local_time_string() -> String {
    #[cfg(target_os = "windows")]
    unsafe {
        #[repr(C)]
        #[derive(Default)]
        struct WinSystemTime {
            year: u16,
            month: u16,
            day_of_week: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            milliseconds: u16,
        }
        extern "system" {
            fn GetLocalTime(lp: *mut WinSystemTime);
        }
        let mut st = WinSystemTime::default();
        GetLocalTime(&mut st);
        return format!("{:02}:{:02}", st.hour, st.minute);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let total_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{:02}:{:02}", (total_secs / 3600) % 24, (total_secs / 60) % 60)
    }
}

// ============================================================================
// ESTRUTURA DE METADADOS E ESTATÍSTICAS
// ============================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HubCategory {
    All,
    Favorites,
    Recents,
}

impl HubCategory {
    pub fn next(self) -> Self {
        match self {
            HubCategory::All => HubCategory::Favorites,
            HubCategory::Favorites => HubCategory::Recents,
            HubCategory::Recents => HubCategory::All,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct GameStats {
    pub play_count: u32,
    pub play_time_seconds: u64,
    pub is_favorite: bool,
    pub last_played_epoch: u64,
}

#[derive(Clone, Debug)]
pub struct RomEntry {
    pub path: String,
    pub raw_name: String,
    pub clean_title: String,
    pub first_char: char,
    pub preview: Vec<u32>,
    pub theme_color: u32,
    pub stats: GameStats,
    pub cover_checked: bool,
}

pub fn sanitize_game_title(name: &str) -> String {
    let mut clean = String::new();
    let mut in_paren = false;
    let mut in_bracket = false;

    for ch in name.chars() {
        if ch == '(' { in_paren = true; continue; }
        if ch == ')' { in_paren = false; continue; }
        if ch == '[' { in_bracket = true; continue; }
        if ch == ']' { in_bracket = false; continue; }

        if !in_paren && !in_bracket {
            if ch == '.' || ch == '_' || ch == '-' {
                clean.push(' ');
            } else {
                clean.push(ch);
            }
        }
    }
    clean.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn compute_theme_color(name: &str) -> u32 {
    let mut hash = 5381u32;
    for b in name.bytes() {
        hash = ((hash << 5).wrapping_add(hash)).wrapping_add(b as u32);
    }
    let r = 80 + ((hash & 0x7F) as u32);
    let g = 100 + (((hash >> 7) & 0x7F) as u32);
    let b = 140 + (((hash >> 14) & 0x7F) as u32);
    (r << 16) | (g << 8) | b
}

pub fn generate_ps1_poster(name: &str, clean_title: &str, theme_color: u32) -> Vec<u32> {
    let mut card = vec![0x10141Eu32; CARD_WIDTH * CARD_HEIGHT];

    for y in 0..CARD_HEIGHT {
        let t = y as f32 / CARD_HEIGHT as f32;
        let r = (((theme_color >> 16) & 0xFF) as f32 * (1.0 - t * 0.7) * 0.25) as u32;
        let g = (((theme_color >> 8) & 0xFF) as f32 * (1.0 - t * 0.7) * 0.25) as u32;
        let b = ((theme_color & 0xFF) as f32 * (1.0 - t * 0.7) * 0.25) as u32;
        let c = (r << 16) | (g << 8) | b;
        for x in 0..CARD_WIDTH {
            card[y * CARD_WIDTH + x] = c;
        }
    }

    draw_rect(&mut card, CARD_WIDTH, CARD_HEIGHT, 0, 0, CARD_WIDTH, CARD_HEIGHT, 0x1A2234);
    draw_rect(&mut card, CARD_WIDTH, CARD_HEIGHT, 1, 1, CARD_WIDTH - 2, CARD_HEIGHT - 2, 0x0D111A);

    // Barra lateral preta original dos jogos de PlayStation 1
    draw_rect(&mut card, CARD_WIDTH, CARD_HEIGHT, 2, 2, 14, CARD_HEIGHT - 4, 0x000000);
    draw_rect(&mut card, CARD_WIDTH, CARD_HEIGHT, 16, 2, 1, CARD_HEIGHT - 4, 0x37474F);

    draw_rect(&mut card, CARD_WIDTH, CARD_HEIGHT, 4, 8, 10, 10, theme_color);
    draw_text(&mut card, CARD_WIDTH, CARD_HEIGHT, "PS", 5, 10, 0x000000);
    draw_text(&mut card, CARD_WIDTH, CARD_HEIGHT, "P", 6, 24, 0xB0BEC5);
    draw_text(&mut card, CARD_WIDTH, CARD_HEIGHT, "S", 6, 34, 0xB0BEC5);
    draw_text(&mut card, CARD_WIDTH, CARD_HEIGHT, "X", 6, 44, 0xB0BEC5);

    let art_x = 20;
    let art_y = 6;
    let art_w = CARD_WIDTH - art_x - 4;
    let art_h = 82;
    draw_rect(&mut card, CARD_WIDTH, CARD_HEIGHT, art_x, art_y, art_w, art_h, 0x131B2A);
    draw_rect(&mut card, CARD_WIDTH, CARD_HEIGHT, art_x, art_y, art_w, 2, theme_color);

    // Tenta carregar imagem local de capa se existir (.png / .jpg)
    let local_png = format!("{}.png", name);
    let cover_png = format!(".covers/{}.png", name);
    let img_path = if Path::new(&local_png).exists() {
        Some(local_png)
    } else if Path::new(&cover_png).exists() {
        Some(cover_png)
    } else {
        None
    };

    if let Some(path_str) = img_path {
        if let Ok(bytes) = fs::read(&path_str) {
            // Se houver arquivo raw ou decodificado
            if bytes.len() >= 90 * 130 * 4 {
                for (i, chunk) in bytes.chunks_exact(4).take(90 * 130).enumerate() {
                    card[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                }
                return card;
            }
        }
    }

    // Gerador de Arte 3D Procedural
    let cx = art_x as f32 + art_w as f32 / 2.0;
    let cy = art_y as f32 + art_h as f32 / 2.0;
    for y in 0..art_h {
        for x in 0..art_w {
            let dx = (art_x + x) as f32 - cx;
            let dy = (art_y + y) as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < 22.0 {
                let shade = ((22.0 - dist) * 7.0) as u32;
                let px = art_x + x;
                let py = art_y + y;
                if px < CARD_WIDTH && py < CARD_HEIGHT {
                    card[py * CARD_WIDTH + px] = 0x263238 | (shade << 16) | (shade << 8);
                }
            }
        }
    }

    draw_rect(&mut card, CARD_WIDTH, CARD_HEIGHT, art_x, 92, art_w, 14, 0x090D15);
    let short_title = if clean_title.len() > 10 { &clean_title[..10] } else { clean_title };
    let tw = short_title.len() * 6;
    let tx = art_x + (art_w.saturating_sub(tw)) / 2;
    draw_text(&mut card, CARD_WIDTH, CARD_HEIGHT, short_title, tx, 96, 0xFFFFFF);

    draw_rect(&mut card, CARD_WIDTH, CARD_HEIGHT, art_x, 112, art_w, 12, 0x1E293B);
    draw_text(&mut card, CARD_WIDTH, CARD_HEIGHT, "PS1 DISC", art_x + 8, 115, theme_color);

    card
}

fn stats_file_path(game_name: &str) -> PathBuf {
    let dir = PathBuf::from(".stats");
    let _ = fs::create_dir_all(&dir);
    dir.join(format!("{}.stats", game_name))
}

pub fn load_game_stats(game_name: &str) -> GameStats {
    let p = stats_file_path(game_name);
    if let Ok(bytes) = fs::read(&p) {
        if bytes.len() >= 24 {
            let play_count = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
            let play_time_seconds = u64::from_le_bytes(bytes[4..12].try_into().unwrap());
            let is_favorite = bytes[12] != 0;
            let last_played_epoch = u64::from_le_bytes(bytes[13..21].try_into().unwrap());
            return GameStats {
                play_count,
                play_time_seconds,
                is_favorite,
                last_played_epoch,
            };
        }
    }
    GameStats::default()
}

pub fn save_game_stats(game_name: &str, stats: &GameStats) {
    let p = stats_file_path(game_name);
    let mut data = Vec::with_capacity(32);
    data.extend_from_slice(&stats.play_count.to_le_bytes());
    data.extend_from_slice(&stats.play_time_seconds.to_le_bytes());
    data.push(if stats.is_favorite { 1 } else { 0 });
    data.extend_from_slice(&stats.last_played_epoch.to_le_bytes());
    let _ = fs::write(&p, &data);
}


// ============================================================================
// DIRETÓRIO OFICIAL DE BIOS EM DOCUMENTOS
// ============================================================================
pub fn get_documents_bios_dir() -> PathBuf {
    let local_bios = PathBuf::from(".").join("bios");
    let _ = fs::create_dir_all(&local_bios);
    ensure_readme_in_bios_dir(&local_bios);

    if let Some(one_drive) = std::env::var_os("OneDrive") {
        let docs = PathBuf::from(&one_drive).join("Documents");
        if docs.is_dir() {
            let bios = docs.join("meu_psx").join("bios");
            let _ = fs::create_dir_all(&bios);
            ensure_readme_in_bios_dir(&bios);
            return bios;
        }
        let docs_pt = PathBuf::from(&one_drive).join("Documentos");
        if docs_pt.is_dir() {
            let bios = docs_pt.join("meu_psx").join("bios");
            let _ = fs::create_dir_all(&bios);
            ensure_readme_in_bios_dir(&bios);
            return bios;
        }
    }

    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let docs = PathBuf::from(&profile).join("Documents");
        if docs.is_dir() {
            let bios = docs.join("meu_psx").join("bios");
            let _ = fs::create_dir_all(&bios);
            ensure_readme_in_bios_dir(&bios);
            return bios;
        }
        let docs_pt = PathBuf::from(&profile).join("Documentos");
        if docs_pt.is_dir() {
            let bios = docs_pt.join("meu_psx").join("bios");
            let _ = fs::create_dir_all(&bios);
            ensure_readme_in_bios_dir(&bios);
            return bios;
        }
    }

    local_bios
}

fn ensure_readme_in_bios_dir(dir: &Path) {
    let readme = dir.join("LEIA-ME_BIOS.txt");
    if !readme.exists() {
        let text = "==================================================================\r\nMEU_PSX - PASTA OFICIAL DE BIOS DO PLAYSTATION 1 (PSX)\r\n==================================================================\r\n\r\nColoque o arquivo de BIOS do PlayStation 1 nesta pasta!\r\n\r\nArquivos compativeis e recomendados:\r\n  * SCPH1001.BIN (PlayStation 1 - America do Norte / NTSC-U) - RECOMENDADO\r\n  * SCPH7001.BIN / SCPH5501.BIN (NTSC-U)\r\n  * SCPH1002.BIN / SCPH7502.BIN (Europa / PAL)\r\n  * SCPH1000.BIN (Japao / NTSC-J)\r\n  * bios.bin\r\n\r\nINFORMACOES IMPORTANTES:\r\n1. O arquivo deve ter exatamente 512 KB (524.288 bytes).\r\n2. O emulador detecta automaticamente qualquer uma dessas BIOS nesta pasta.\r\n3. Voce tambem pode abrir esta pasta diretamente pelo menu de opcoes do emulador.\r\n\r\n==================================================================\r\n";
        let _ = fs::write(readme, text);
    }
}

// ============================================================================
// CONFIGURAÇÕES DO EMULADOR E IDIOMA
// ============================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    PtBr,
    PtPt,
    Es,
    En,
}

impl Language {
    pub fn next(self) -> Self {
        match self {
            Self::PtBr => Self::PtPt,
            Self::PtPt => Self::Es,
            Self::Es => Self::En,
            Self::En => Self::PtBr,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::PtBr => Self::En,
            Self::PtPt => Self::PtBr,
            Self::Es => Self::PtPt,
            Self::En => Self::Es,
        }
    }

    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::PtBr,
            1 => Self::PtPt,
            2 => Self::Es,
            _ => Self::En,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EmulatorSettings {
    pub language: Language,
    pub welcome_completed: bool,
    pub display_mode: DisplayMode,
    pub aspect_mode: AspectMode,
    pub internal_res: crate::gpu::InternalResolution,
    pub save_slot: usize,
    pub audio_vol: u32,
}

impl Default for EmulatorSettings {
    fn default() -> Self {
        Self {
            language: Language::PtBr,
            welcome_completed: false,
            display_mode: DisplayMode::Windowed,
            aspect_mode: AspectMode::Classic4x3,
            internal_res: crate::gpu::InternalResolution::Native1x,
            save_slot: 0,
            audio_vol: 80,
        }
    }
}

impl EmulatorSettings {
    pub fn load() -> Self {
        if let Ok(bytes) = std::fs::read(".emulator_settings.bin") {
            if bytes.len() >= 6 {
                let language = Language::from_u8(bytes[0]);
                let welcome_completed = bytes[1] != 0;
                let display_mode = match bytes[2] % 3 {
                    0 => DisplayMode::Windowed,
                    1 => DisplayMode::Borderless,
                    _ => DisplayMode::Fullscreen,
                };
                let aspect_mode = match bytes[3] % 3 {
                    0 => AspectMode::Classic4x3,
                    1 => AspectMode::WidescreenHack,
                    _ => AspectMode::WidescreenStretch,
                };
                let save_slot = (bytes[4] as usize) % 5;
                let audio_vol = (bytes[5] as u32).min(100);
                return Self {
                    language,
                    welcome_completed,
                    display_mode,
                    aspect_mode,
                    internal_res: crate::gpu::InternalResolution::Native1x,
                    save_slot,
                    audio_vol,
                };
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let bytes = [
            self.language as u8,
            self.welcome_completed as u8,
            self.display_mode as u8,
            self.aspect_mode as u8,
            self.save_slot as u8,
            self.audio_vol as u8,
        ];
        let _ = std::fs::write(".emulator_settings.bin", bytes);
    }
}

// ============================================================================
// TELA DE BOAS-VINDAS (WELCOME SCREEN)
// ============================================================================
pub fn render_welcome_screen(
    buffer: &mut [u32],
    selected_lang: Language,
    gamepad_connected: bool,
) {
    for y in 0..CANVAS_HEIGHT {
        let factor = y as f32 / CANVAS_HEIGHT as f32;
        let r = (10.0 + 8.0 * (1.0 - factor)) as u32;
        let g = (15.0 + 12.0 * (1.0 - factor)) as u32;
        let b = (26.0 + 20.0 * (1.0 - factor)) as u32;
        let col = (r << 16) | (g << 8) | b;
        for x in 0..CANVAS_WIDTH {
            buffer[y * CANVAS_WIDTH + x] = col;
        }
    }

    for y in (0..CANVAS_HEIGHT).step_by(16) {
        for x in (0..CANVAS_WIDTH).step_by(16) {
            buffer[y * CANVAS_WIDTH + x] = 0x1E283A;
        }
    }

    let mw = 380;
    let mh = 230;
    let mx = (CANVAS_WIDTH - mw) / 2;
    let my = (CANVAS_HEIGHT - mh) / 2;

    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx - 1, my - 1, mw + 2, mh + 2, 0x00E5FF);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, my, mw, mh, 0x0B101B);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, my, mw, 30, 0x141F33);

    let (title, sub, btn_txt) = match selected_lang {
        Language::PtBr => (
            "BEM-VINDO AO ARCADE PLAYSTATION 1",
            "ESCOLHA O IDIOMA DO SEU EMULADOR",
            "CONFIRMAR E INICIAR",
        ),
        Language::PtPt => (
            "BEM-VINDO AO ARCADE PLAYSTATION 1",
            "SELECIONE O IDIOMA DO EMULADOR",
            "CONFIRMAR E INICIAR",
        ),
        Language::Es => (
            "BIENVENIDO A ARCADE PLAYSTATION 1",
            "SELECCIONA EL IDIOMA DEL SISTEMA",
            "CONFIRMAR E INICIAR",
        ),
        Language::En => (
            "WELCOME TO ARCADE PLAYSTATION 1",
            "SELECT YOUR SYSTEM LANGUAGE",
            "CONFIRM AND START",
        ),
    };

    draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, mw, my + 11, title, 0x00E5FF);
    draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, mw, my + 38, sub, 0xCFD8DC);

    let langs = [
        (Language::PtBr, "PORTUGUES (BRASIL) - PADRAO"),
        (Language::PtPt, "PORTUGUES (PORTUGAL)"),
        (Language::Es, "ESPANOL"),
        (Language::En, "ENGLISH"),
    ];

    for (i, &(l_enum, l_name)) in langs.iter().enumerate() {
        let ry = my + 54 + i * 26;
        let is_sel = selected_lang == l_enum;

        draw_rect(
            buffer,
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            mx + 30,
            ry,
            mw - 60,
            20,
            if is_sel { 0x00E5FF } else { 0x152033 },
        );
        draw_text_centered(
            buffer,
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            mx + 30,
            mw - 60,
            ry + 7,
            l_name,
            if is_sel { 0x000000 } else { 0xFFFFFF },
        );
    }

    let info_y = my + 162;
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx + 20, info_y, mw - 40, 26, 0x101726);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx + 20, info_y, mw - 40, 1, 0x1E2B45);
    draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, mw, info_y + 4, "PASTAS CRIADAS AUTOMATICAMENTE:", 0x00E5FF);
    draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, mw, info_y + 15, r"Documentos\meu_psx\isos  e  meu_psx\bios", 0x81C784);

    let foot_y = my + mh - 22;
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, foot_y - 6, mw, 1, 0x1E2B42);
    if gamepad_connected {
        draw_centered_badge_row(
            buffer,
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            mx,
            mw,
            foot_y - 2,
            12,
            &[
                ("D-PAD", 0x37474F, 0xFFFFFF, "ESCOLHER"),
                ("A", 0x2E7D32, 0xFFFFFF, btn_txt),
            ],
        );
    } else {
        let kb_str = format!("CIMA/BAIXO: ESCOLHER   ENTER: {}", btn_txt);
        draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, mw, foot_y + 1, &kb_str, 0x00E5FF);
    }
}
// ============================================================================
// DIRETÓRIO OFICIAL DE JOGOS E ISOS EM DOCUMENTOS
// ============================================================================
pub fn get_documents_isos_dir() -> PathBuf {
    if let Some(one_drive) = std::env::var_os("OneDrive") {
        let docs = PathBuf::from(&one_drive).join("Documents");
        if docs.is_dir() {
            let isos = docs.join("meu_psx").join("isos");
            let _ = fs::create_dir_all(&isos);
            ensure_readme_in_isos_dir(&isos);
            return isos;
        }
        let docs_pt = PathBuf::from(&one_drive).join("Documentos");
        if docs_pt.is_dir() {
            let isos = docs_pt.join("meu_psx").join("isos");
            let _ = fs::create_dir_all(&isos);
            ensure_readme_in_isos_dir(&isos);
            return isos;
        }
    }

    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let docs = PathBuf::from(&profile).join("Documents");
        if docs.is_dir() {
            let isos = docs.join("meu_psx").join("isos");
            let _ = fs::create_dir_all(&isos);
            ensure_readme_in_isos_dir(&isos);
            return isos;
        }
        let docs_pt = PathBuf::from(&profile).join("Documentos");
        if docs_pt.is_dir() {
            let isos = docs_pt.join("meu_psx").join("isos");
            let _ = fs::create_dir_all(&isos);
            ensure_readme_in_isos_dir(&isos);
            return isos;
        }
    }

    let isos = PathBuf::from(".").join("isos");
    let _ = fs::create_dir_all(&isos);
    ensure_readme_in_isos_dir(&isos);
    isos
}

fn ensure_readme_in_isos_dir(dir: &Path) {
    let readme = dir.join("LEIA-ME_ISOS.txt");
    if !readme.exists() {
        let text = "==================================================================\r\nMEU_PSX - PASTA OFICIAL DE JOGOS E ISOS DO PLAYSTATION 1\r\n==================================================================\r\n\r\nColoque suas imagens de jogos do PS1 nesta pasta!\r\n\r\nFormatos suportados:\r\n  * .cue (com os arquivos .bin da trilha na mesma pasta)\r\n  * .bin / .iso / .img\r\n  * .pbp (jogos convertidos do PS1 / eBoot)\r\n  * .chd (formato compactado)\r\n  * .exe (executaveis PSX / homebrews)\r\n\r\nDICAS E RECURSOS:\r\n1. Voce pode colocar as ISOs diretamente aqui ou dentro de subpastas\r\n   (exemplo: 'isos\\Tekken 3\\Tekken 3.cue').\r\n2. As capas oficiais sao baixadas automaticamente em segundo plano\r\n   atraves do banco de dados oficial do Libretro!\r\n3. Pressione [Y] no joystick Xbox ou [F1] no teclado no Hub Arcade\r\n   para abrir esta pasta a qualquer momento no Windows Explorer.\r\n\r\n==================================================================\r\n";
        let _ = fs::write(readme, text);
    }
}

pub fn get_cover_cache_dir() -> PathBuf {
    let dir = PathBuf::from(".covers");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn load_cached_raw_cover(clean_title: &str) -> Option<Vec<u32>> {
    let cache_file = get_cover_cache_dir().join(format!("{}.raw", clean_title.replace(' ', "_")));
    if let Ok(bytes) = fs::read(&cache_file) {
        if bytes.len() == CARD_WIDTH * CARD_HEIGHT * 4 {
            let mut raw_pixels = vec![0u32; CARD_WIDTH * CARD_HEIGHT];
            for (i, chunk) in bytes.chunks_exact(4).enumerate() {
                raw_pixels[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            }
            return Some(raw_pixels);
        }
    }
    None
}

fn urlencoding_simple(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// ============================================================================
// DOWNLOAD ROBUSTO DE CAPAS OFICIAIS DO PS1 (LIBRETRO MULTI-FONTE)
// ============================================================================
pub fn download_or_load_boxart(rom_path: &str, rom_stem: &str, target_w: usize, target_h: usize) -> Option<Vec<u32>> {
    let clean_title = sanitize_game_title(rom_stem);
    let cache_dir = get_cover_cache_dir();
    let cache_file = cache_dir.join(format!("{}.raw", clean_title.replace(' ', "_")));

    // 1. CARREGA DO CACHE LOCAL (.covers/*.raw)
    if let Ok(bytes) = fs::read(&cache_file) {
        if bytes.len() == target_w * target_h * 4 {
            let mut raw_pixels = vec![0u32; target_w * target_h];
            for (i, chunk) in bytes.chunks_exact(4).enumerate() {
                raw_pixels[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            }
            return Some(raw_pixels);
        }
    }

    // 2. TENTA CAPA LOCAL CASO EXISTA (.png/.jpg no diretorio da ROM)
    let img_extensions = ["png", "jpg", "jpeg", "webp"];
    let rom_path_buf = PathBuf::from(rom_path);
    let parent_dir = rom_path_buf.parent();

    for ext in &img_extensions {
        let local_img = rom_path_buf.with_extension(ext);
        if local_img.exists() {
            if let Ok(img) = image::open(&local_img) {
                let resized = img.resize_exact(target_w as u32, target_h as u32, image::imageops::FilterType::Lanczos3);
                let rgba = resized.to_rgba8();
                let mut pixels = vec![0u32; target_w * target_h];
                let mut byte_cache = Vec::with_capacity(target_w * target_h * 4);
                for (i, p) in rgba.pixels().enumerate() {
                    let col = ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32);
                    pixels[i] = col;
                    byte_cache.extend_from_slice(&col.to_le_bytes());
                }
                let _ = fs::write(&cache_file, byte_cache);
                return Some(pixels);
            }
        }

        if let Some(p) = parent_dir {
            for name in &["cover", "folder", "front", rom_stem] {
                let local_img = p.join(format!("{}.{}", name, ext));
                if local_img.exists() {
                    if let Ok(img) = image::open(&local_img) {
                        let resized = img.resize_exact(target_w as u32, target_h as u32, image::imageops::FilterType::Lanczos3);
                        let rgba = resized.to_rgba8();
                        let mut pixels = vec![0u32; target_w * target_h];
                        let mut byte_cache = Vec::with_capacity(target_w * target_h * 4);
                        for (i, p) in rgba.pixels().enumerate() {
                            let col = ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32);
                            pixels[i] = col;
                            byte_cache.extend_from_slice(&col.to_le_bytes());
                        }
                        let _ = fs::write(&cache_file, byte_cache);
                        return Some(pixels);
                    }
                }
            }
        }
    }

    // 3. BUSCA OFICIAL NO REPOSITÓRIO SONY PLAYSTATION DO LIBRETRO
    let mut candidates = vec![
        format!("{} (USA)", clean_title),
        clean_title.clone(),
        format!("{} (Europe)", clean_title),
        format!("{} (Japan)", clean_title),
        rom_stem.trim().to_string(),
    ];

    if clean_title.contains(" 2") {
        candidates.push(clean_title.replace(" 2", " II (USA)"));
    } else if clean_title.contains(" 3") {
        candidates.push(clean_title.replace(" 3", " III (USA)"));
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_millis(3500))
        .build();

    for title in &candidates {
        let encoded = urlencoding_simple(title);
        let urls = [
            format!("https://raw.githubusercontent.com/libretro-thumbnails/Sony_-_PlayStation/master/Named_Boxarts/{}.png", encoded),
            format!("http://thumbnails.libretro.com/Sony%20-%20PlayStation/Named_Boxarts/{}.png", encoded),
        ];

        for url in &urls {
            if let Ok(response) = agent.get(url).call() {
                let mut img_bytes = Vec::new();
                if response.into_reader().read_to_end(&mut img_bytes).is_ok() {
                    if let Ok(img) = image::load_from_memory(&img_bytes) {
                        let resized = img.resize_exact(target_w as u32, target_h as u32, image::imageops::FilterType::Lanczos3);
                        let rgba = resized.to_rgba8();
                        let mut pixels = vec![0u32; target_w * target_h];
                        let mut byte_cache = Vec::with_capacity(target_w * target_h * 4);

                        for (i, p) in rgba.pixels().enumerate() {
                            let col = ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32);
                            pixels[i] = col;
                            byte_cache.extend_from_slice(&col.to_le_bytes());
                        }

                        let _ = fs::write(&cache_file, byte_cache);
                        return Some(pixels);
                    }
                }
            }
        }
    }

    None
}

// ============================================================================
// CARREGADOR ASSÍNCRONO EM SEGUNDO PLANO (ZERO IMPACTO NO FRAME RATE)
// ============================================================================
pub struct AsyncCoverLoader {
    req_tx: Sender<(String, String)>,
    res_rx: Receiver<(String, Vec<u32>)>,
}

impl AsyncCoverLoader {
    pub fn new() -> Self {
        let (req_tx, req_rx) = channel::<(String, String)>();
        let (res_tx, res_rx) = channel::<(String, Vec<u32>)>();

        thread::spawn(move || {
            while let Ok((rom_path, rom_stem)) = req_rx.recv() {
                if let Some(pixels) = download_or_load_boxart(&rom_path, &rom_stem, CARD_WIDTH, CARD_HEIGHT) {
                    let _ = res_tx.send((rom_path, pixels));
                }
            }
        });

        Self { req_tx, res_rx }
    }

    pub fn poll_and_request(&self, games: &mut [RomEntry], visible_indices: &[usize]) {
        while let Ok((path, pixels)) = self.res_rx.try_recv() {
            if let Some(game) = games.iter_mut().find(|g| g.path == path) {
                game.preview = pixels;
            }
        }

        for &idx in visible_indices {
            if idx < games.len() && !games[idx].cover_checked {
                games[idx].cover_checked = true;
                let _ = self.req_tx.send((games[idx].path.clone(), games[idx].raw_name.clone()));
            }
        }
    }
}

pub fn scan_games() -> Vec<RomEntry> {
    let mut list = Vec::new();
    let doc_isos = get_documents_isos_dir();
    let doc_games = doc_isos.parent().map(|p| p.join("games")).unwrap_or_else(|| PathBuf::from("games"));

    let search_roots = [
        doc_isos,
        doc_games,
        PathBuf::from("."),
        PathBuf::from("./isos"),
        PathBuf::from("./games"),
        PathBuf::from("./roms"),
    ];

    let mut scanned_paths = std::collections::HashSet::new();
    let mut scanned_cues = Vec::new();

    // Coleta arquivos de diretórios principais e de subdiretórios (1 nível)
    let mut all_files = Vec::new();
    for root in &search_roots {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    if let Ok(sub_entries) = fs::read_dir(&p) {
                        for sub in sub_entries.flatten() {
                            let sp = sub.path();
                            if sp.is_file() {
                                all_files.push(sp);
                            }
                        }
                    }
                } else if p.is_file() {
                    all_files.push(p);
                }
            }
        }
    }

    // 1. Processa arquivos .CUE primeiro (são os masters dos jogos multi-track de PS1)
    for path in &all_files {
        let p_str = path.to_string_lossy().to_lowercase();
        if p_str.contains("target") || p_str.contains(".covers") || p_str.contains(".stats") || p_str.contains(".git") {
            continue;
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("cue") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let file_size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                    if is_system_or_emulator_file(stem, ext, file_size) {
                        continue;
                    }
                    let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
                    if scanned_paths.contains(&canon) {
                        continue;
                    }
                    scanned_paths.insert(canon);
                    scanned_cues.push(stem.to_lowercase());

                    let clean = sanitize_game_title(stem);
                    let theme = compute_theme_color(stem);
                    let mut cover_checked = false;
                    let preview = if let Some(cached) = load_cached_raw_cover(&clean) {
                        cover_checked = true;
                        cached
                    } else {
                        generate_ps1_poster(stem, &clean, theme)
                    };

                    let first_c = clean.chars().next().unwrap_or('#').to_ascii_uppercase();
                    let first_char = if first_c.is_alphabetic() { first_c } else { '#' };
                    let stats = load_game_stats(stem);

                    list.push(RomEntry {
                        path: path.to_string_lossy().into_owned(),
                        raw_name: stem.to_string(),
                        clean_title: clean,
                        first_char,
                        preview,
                        theme_color: theme,
                        stats,
                        cover_checked,
                    });
                }
            }
        }
    }

    // 2. Processa arquivos .BIN, .ISO, .IMG, .PBP, .CHD individuais (ignora executáveis do sistema e BIOS)
    for path in &all_files {
        let p_str = path.to_string_lossy().to_lowercase();
        if p_str.contains("target") || p_str.contains(".covers") || p_str.contains(".stats") || p_str.contains(".git") {
            continue;
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();
            if ext_lower == "bin" || ext_lower == "iso" || ext_lower == "img"
                || ext_lower == "pbp" || ext_lower == "chd" {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let file_size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                    if is_system_or_emulator_file(stem, ext, file_size) {
                        continue;
                    }
                    let stem_lower = stem.to_lowercase();
                    let is_covered = scanned_cues.iter().any(|c| stem_lower.starts_with(c));
                    if is_covered {
                        continue;
                    }

                    let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
                    if scanned_paths.contains(&canon) {
                        continue;
                    }
                    scanned_paths.insert(canon);

                    let clean = sanitize_game_title(stem);
                    let theme = compute_theme_color(stem);
                    let mut cover_checked = false;
                    let preview = if let Some(cached) = load_cached_raw_cover(&clean) {
                        cover_checked = true;
                        cached
                    } else {
                        generate_ps1_poster(stem, &clean, theme)
                    };

                    let first_c = clean.chars().next().unwrap_or('#').to_ascii_uppercase();
                    let first_char = if first_c.is_alphabetic() { first_c } else { '#' };
                    let stats = load_game_stats(stem);

                    list.push(RomEntry {
                        path: path.to_string_lossy().into_owned(),
                        raw_name: stem.to_string(),
                        clean_title: clean,
                        first_char,
                        preview,
                        theme_color: theme,
                        stats,
                        cover_checked,
                    });
                }
            }
        }
    }

    list.sort_by(|a, b| a.clean_title.to_lowercase().cmp(&b.clean_title.to_lowercase()));
    list
}

fn draw_rounded_card(
    buffer: &mut [u32],
    card_x: i32,
    card_y: i32,
    card_w: usize,
    card_h: usize,
    preview: &[u32],
    is_focused: bool,
    theme_color: u32,
) {
    let border_color = if is_focused { 0x00E5FF } else { 0x242D3D };

    for y in 0..card_h {
        let dy = card_y + y as i32;
        if dy < 0 || dy >= CANVAS_HEIGHT as i32 { continue; }

        for x in 0..card_w {
            let dx = card_x + x as i32;
            if dx < 0 || dx >= CANVAS_WIDTH as i32 { continue; }

            let is_border = x == 0 || x == card_w - 1 || y == 0 || y == card_h - 1;
            let is_inner = is_focused && (x == 1 || x == card_w - 2 || y == 1 || y == card_h - 2);

            let color = if is_border {
                border_color
            } else if is_inner {
                theme_color
            } else {
                let sample_x = (x * 90) / card_w;
                let sample_y = (y * 130) / card_h;
                let pixel = preview[sample_y * 90 + sample_x];
                if is_focused {
                    pixel
                } else {
                    let r = (((pixel >> 16) & 0xFF) * 4) / 10;
                    let g = (((pixel >> 8) & 0xFF) * 4) / 10;
                    let b = ((pixel & 0xFF) * 4) / 10;
                    (r << 16) | (g << 8) | b
                }
            };

            buffer[(dy as usize) * CANVAS_WIDTH + (dx as usize)] = color;
        }
    }
}

pub const VIRTUAL_KB_CHARS: &[&str] = &[
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J",
    "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T",
    "U", "V", "W", "X", "Y", "Z", "0", "1", "2", "3",
    "4", "5", "6", "7", "8", "9", "ESPACO", "APAGAR", "LIMPAR", "OK",
];

pub struct HubState {
    pub category: HubCategory,
    pub selected_pos: usize,
    pub filtered_indices: Vec<usize>,
    pub in_search_mode: bool,
    pub search_query: String,
    pub vkb_cursor: usize,
    pub in_settings_menu: bool,
    pub settings_tab: usize,
    pub settings_row: usize,
    pub in_detail_card: bool,
    pub detail_cursor: usize,
}

impl HubState {
    pub fn new(total_games: usize) -> Self {
        let indices = (0..total_games).collect();
        Self {
            category: HubCategory::All,
            selected_pos: 0,
            filtered_indices: indices,
            in_search_mode: false,
            search_query: String::new(),
            vkb_cursor: 0,
            in_settings_menu: false,
            settings_tab: 0,
            settings_row: 0,
            in_detail_card: false,
            detail_cursor: 0,
        }
    }

    pub fn rebuild_filter(&mut self, games: &[RomEntry]) {
        let q = self.search_query.trim().to_uppercase();
        self.filtered_indices = games.iter().enumerate().filter(|(_, g)| {
            let match_q = q.is_empty() || g.clean_title.to_uppercase().contains(&q);
            let match_cat = match self.category {
                HubCategory::All => true,
                HubCategory::Favorites => g.stats.is_favorite,
                HubCategory::Recents => g.stats.play_count > 0,
            };
            match_q && match_cat
        }).map(|(i, _)| i).collect();

        if self.category == HubCategory::Recents {
            self.filtered_indices.sort_by(|&a, &b| games[b].stats.last_played_epoch.cmp(&games[a].stats.last_played_epoch));
        }

        if !self.filtered_indices.is_empty() && self.selected_pos >= self.filtered_indices.len() {
            self.selected_pos = self.filtered_indices.len() - 1;
        }
    }
}

pub fn render_arcade_hub_frame(
    buffer: &mut [u32],
    games: &[RomEntry],
    state: &HubState,
    gamepad_connected: bool,
) {
    let active_theme = if !state.filtered_indices.is_empty() {
        games[state.filtered_indices[state.selected_pos]].theme_color
    } else {
        0x00E5FF
    };

    let tr = ((active_theme >> 16) & 0xFF) as f32;
    let tg = ((active_theme >> 8) & 0xFF) as f32;
    let tb = (active_theme & 0xFF) as f32;

    for y in 0..CANVAS_HEIGHT {
        let factor = (y as f32 / CANVAS_HEIGHT as f32) * 0.7;
        let r = ((tr * 0.15) * (1.0 - factor) + 10.0 * factor) as u32;
        let g = ((tg * 0.15) * (1.0 - factor) + 14.0 * factor) as u32;
        let b = ((tb * 0.18) * (1.0 - factor) + 24.0 * factor) as u32;
        let bg_col = (r << 16) | (g << 8) | b;
        for x in 0..CANVAS_WIDTH {
            buffer[y * CANVAS_WIDTH + x] = bg_col;
        }
    }

    for y in (44..CANVAS_HEIGHT).step_by(16) {
        for x in (0..CANVAS_WIDTH).step_by(16) {
            buffer[y * CANVAS_WIDTH + x] = 0x222C3E;
        }
    }

    // 1. BARRA SUPERIOR
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, 0, 0, CANVAS_WIDTH, 24, 0x0C101A);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, 12, 5, 5, 5, 0x00E5FF);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, 19, 5, 5, 5, 0x00E5FF);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, 12, 12, 5, 5, 0x00E5FF);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, 19, 12, 5, 5, 0x00E5FF);
    draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, "ARCADE LIBRARY", 30, 8, 0xFFFFFF);

    let cat_label = match state.category {
        HubCategory::All => "TODOS",
        HubCategory::Favorites => "FAVORITOS",
        HubCategory::Recents => "RECENTES",
    };

    let count_str = if state.search_query.is_empty() {
        format!("{}: {}/{}", cat_label, if state.filtered_indices.is_empty() { 0 } else { state.selected_pos + 1 }, state.filtered_indices.len())
    } else {
        format!("{} '{}': {}/{}", cat_label, state.search_query, if state.filtered_indices.is_empty() { 0 } else { state.selected_pos + 1 }, state.filtered_indices.len())
    };
    draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, 0, CANVAS_WIDTH, 8, &count_str, 0x00E5FF);

    let pad_color = if gamepad_connected { 0x81C784 } else { 0x455A64 };
    draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, "XBOX PAD", 400, 8, pad_color);
    let time_str = get_local_time_string();
    draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, &time_str, 468, 8, 0xFFFFFF);

    // 2. BARRA ALFABÉTICA CENTRALIZADA
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, 0, 24, CANVAS_WIDTH, 16, 0x111726);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, 0, 40, CANVAS_WIDTH, 1, 0x1E273A);

    let current_letter = if !state.filtered_indices.is_empty() {
        games[state.filtered_indices[state.selected_pos]].first_char
    } else {
        ' '
    };

    let alphabet = "#ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let start_alpha_x = 42;
    draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, "LB<", 10, 29, 0x607D8B);
    for (i, ch) in alphabet.chars().enumerate() {
        let box_x = start_alpha_x + i * 16;
        let is_cur = ch == current_letter;
        if is_cur {
            draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, box_x - 3, 26, 11, 12, 0x00E5FF);
        }
        let s = ch.to_string();
        draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, &s, box_x, 29, if is_cur { 0x000000 } else { 0x78909C });
    }
    draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, ">RB", 484, 29, 0x607D8B);

    // 3. CARROSSEL DE CARDS
    if state.filtered_indices.is_empty() {
        let mw = 320;
        let mx = (CANVAS_WIDTH - mw) / 2;
        draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, 95, mw, 100, 0x151B26);
        draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, 95, mw, 2, 0x00E5FF);
        if games.is_empty() {
            draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, mw, 115, "NENHUM JOGO ENCONTRADO", 0x00E5FF);
            draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, mw, 140, "COLOQUE ARQUIVOS .BIN/.CUE/.ISO NA PASTA", 0xFFFFFF);
            draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, mw, 160, "DO EMULADOR OU NA PASTA 'GAMES'", 0x90A4AE);
        } else {
            draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, mw, 125, "NENHUM RESULTADO NA BUSCA", 0xFF5252);
            draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, mw, 150, "PRESSIONE X / F2 PARA DIGITAR OUTRO NOME", 0xCFD8DC);
        }
    } else {
        let center_card_w = 100;
        let center_card_h = 140;
        let center_card_x = (CANVAS_WIDTH as i32 - center_card_w as i32) / 2;
        let center_card_y = 48;

        let side_card_w = 76;
        let side_card_h = 108;
        let side_card_y = 64;

        let total = state.filtered_indices.len() as i32;
        let sel = state.selected_pos as i32;

        for offset in [-2, -1, 1, 2] {
            let pos = sel + offset;
            if pos >= 0 && pos < total {
                let rom_idx = state.filtered_indices[pos as usize];
                let card_x = if offset < 0 {
                    center_card_x - 16 - (side_card_w as i32) + (offset + 1) * 90
                } else {
                    center_card_x + (center_card_w as i32) + 16 + (offset - 1) * 90
                };
                draw_rounded_card(
                    buffer, card_x, side_card_y, side_card_w, side_card_h,
                    &games[rom_idx].preview, false, games[rom_idx].theme_color,
                );
            }
        }

        let sel_rom = &games[state.filtered_indices[state.selected_pos]];
        draw_rounded_card(
            buffer, center_card_x, center_card_y, center_card_w, center_card_h,
            &sel_rom.preview, true, active_theme,
        );

        if sel_rom.stats.is_favorite {
            draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, center_card_x as usize + 58, center_card_y as usize + 6, 36, 12, 0xFFD54F);
            draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, center_card_x as usize + 58, 36, center_card_y as usize + 9, "*FAV", 0x000000);
        }

        draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, 0, CANVAS_WIDTH, 202, &sel_rom.clean_title, 0xFFFFFF);

        let btn_w = 156;
        let btn_x = (CANVAS_WIDTH - btn_w) / 2;
        draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, btn_x, 224, btn_w, 22, 0x1A2538);
        draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, btn_x, 224, btn_w, 1, 0x00E5FF);
        draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, btn_x, 245, btn_w, 1, 0x00E5FF);

        if gamepad_connected {
            let bw = badge_pixel_width("A");
            let start_x = btn_x + (btn_w.saturating_sub(bw + 5 + "ABRIR JOGO".len() * 6)) / 2;
            draw_button_badge(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, "A", start_x, 229, 0x2E7D32, 0xFFFFFF);
            draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, "ABRIR JOGO", start_x + bw + 5, 231, 0xFFFFFF);
        } else {
            draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, btn_x, btn_w, 231, "ENTER: ABRIR JOGO", 0xFFFFFF);
        }
    }

    // 4. TECLADO VIRTUAL DE BUSCA (MODAL)
    if state.in_search_mode {
        let mw = 390;
        let mh = 135;
        let mx = (CANVAS_WIDTH - mw) / 2;
        let my = 80;

        draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx - 1, my - 1, mw + 2, mh + 2, 0x00E5FF);
        draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, my, mw, mh, 0x0B101B);
        draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, my, mw, 28, 0x141E30);

        let prompt = format!("BUSCAR: {}_", state.search_query);
        draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, &prompt, mx + 14, my + 10, 0x00E5FF);

        let cols = 10;
        let grid_start_x = mx + (mw - 367) / 2;
        for (i, &key_label) in VIRTUAL_KB_CHARS.iter().enumerate() {
            let r = i / cols;
            let c = i % cols;
            let kx = grid_start_x + c * 37;
            let ky = my + 38 + r * 22;

            let is_sel = i == state.vkb_cursor;
            draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, kx, ky, 34, 18, if is_sel { 0x00E5FF } else { 0x152033 });
            let col = if is_sel { 0x000000 } else { 0xFFFFFF };
            draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, kx, 34, ky + 5, key_label, col);
        }
    }

    // 5. BARRA DE ATALHOS NO RODAPÉ IDÊNTICA AO PENGINE
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, 0, 268, CANVAS_WIDTH, 20, 0x0A0D14);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, 0, 268, CANVAS_WIDTH, 1, 0x1A2234);

    if gamepad_connected {
        draw_centered_badge_row(
            buffer,
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            0,
            CANVAS_WIDTH,
            273,
            9,
            &[
                ("LB/RB", 0x37474F, 0xFFFFFF, "A-Z"),
                ("LT", 0x37474F, 0xFFFFFF, "ABA"),
                ("SELECT", 0x37474F, 0xFFFFFF, "FAV"),
                ("X", 0x1565C0, 0xFFFFFF, "BUSCA"),
                ("Y", 0xF57F17, 0xFFFFFF, "PASTA"),
                ("START", 0x1565C0, 0xFFFFFF, "MENU"),
                ("B", 0xC62828, 0xFFFFFF, "SAIR"),
            ],
        );
    } else {
        draw_text_centered(
            buffer,
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            0,
            CANVAS_WIDTH,
            274,
            "PGUP/DN: A-Z   TAB: ABA   F: FAV   F2: BUSCA   F1: PASTA   F3: MENU",
            0x78909C,
        );
    }
}

// ============================================================================
// MODAL DE CONFIGURAÇÕES AVANÇADAS (MENU POR ABAS - F3 / START NO HUB)
// ============================================================================
pub fn render_advanced_settings_menu(
    buffer: &mut [u32],
    active_tab: usize,
    row_cursor: usize,
    gamepad_connected: bool,
    config: &crate::gpu::GpuConfig,
    display_mode: DisplayMode,
    aspect_mode: AspectMode,
    save_slot: usize,
    audio_vol: u32,
    cpu_clock_mode: u8,
    rumble_strength: u8,
) {
    for px in buffer.iter_mut() {
        let p = *px;
        let r = (((p >> 16) & 0xFF) * 2) / 10;
        let g = (((p >> 8) & 0xFF) * 2) / 10;
        let b = ((p & 0xFF) * 2) / 10;
        *px = (r << 16) | (g << 8) | b;
    }

    let mw = 420;
    let mh = 236;
    let mx = (CANVAS_WIDTH - mw) / 2;
    let my = (CANVAS_HEIGHT - mh) / 2;

    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx - 1, my - 1, mw + 2, mh + 2, 0x00E5FF);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, my, mw, mh, 0x0B0F19);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, my, mw, 26, 0x131A29);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, my + 26, mw, 1, 0x1E2B42);

    let tabs = ["1.VIDEO", "2.AUDIO", "3.SISTEMA", "4.CONTROLE"];
    let tab_w = mw / tabs.len();

    for (t_idx, &t_name) in tabs.iter().enumerate() {
        let tx = mx + t_idx * tab_w;
        let is_tab_sel = t_idx == active_tab;

        if is_tab_sel {
            draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, tx, my + 1, tab_w, 25, 0x1D2A42);
            draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, tx, my + 24, tab_w, 2, 0x00E5FF);
        }
        let tc = if is_tab_sel { 0x00E5FF } else { 0x607D8B };
        draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, tx, tab_w, my + 10, t_name, tc);
    }

    let items: Vec<(&str, String)> = match active_tab {
        0 => vec![
            ("MODO DE TELA", match display_mode {
                DisplayMode::Windowed => "< JANELA (1024x576) >".into(),
                DisplayMode::Borderless => "< SEM BORDAS >".into(),
                DisplayMode::Fullscreen => "< TELA CHEIA >".into(),
            }),
            ("FORMATO DE TELA", match aspect_mode {
                AspectMode::Classic4x3 => "< 4:3 ORIGINAL (BORDAS) >".into(),
                AspectMode::WidescreenHack => "< 16:9 HACK (FOV 3D REAL) >".into(),
                AspectMode::WidescreenStretch => "< 16:9 ESTICADO (FULL) >".into(),
            }),
            ("RESOLUCAO INTERNA", match config.internal_res {
                crate::gpu::InternalResolution::Native1x => "< 1x NATIVA (240p) >".into(),
                crate::gpu::InternalResolution::Hd2x => "< 2x HD (480p) >".into(),
                crate::gpu::InternalResolution::FullHd4x => "< 4x FULL HD (960p) >".into(),
            }),
            ("PRECISAO PGXP (SUBPIXEL)", if config.pgxp_enabled { "< ATIVADO (ANTI-TREMOR) >".into() } else { "< DESATIVADO >".into() }),
            ("TEXTURA PERSPECTIVA", if config.perspective_correct { "< ATIVADO (SEM DISTORCAO) >".into() } else { "< DESATIVADO >".into() }),
            ("SUAVIZACAO TEXTURA", if config.bilinear_filter { "< BILINEAR SUAVE >".into() } else { "< NITIDO (ORIGINAL) >".into() }),
            ("EFEITO DE TELA (CRT)", match config.shader {
                crate::gpu::ShaderEffect::None => "< DESATIVADO (HD LIMPO) >".into(),
                crate::gpu::ShaderEffect::Bilinear => "< BILINEAR >".into(),
                crate::gpu::ShaderEffect::CrtScanlines => "< CRT SCANLINES >".into(),
                crate::gpu::ShaderEffect::BilinearCrt => "< BILINEAR + CRT >".into(),
                crate::gpu::ShaderEffect::RfNoise => "< TV ANTIGA RF (CHIADO) >".into(),
            }),
        ],
        1 => vec![
            ("VOLUME GERAL", format!("< {}% >", audio_vol)),
            ("INTERPOLACAO SPU", "< GAUSSIANA (PS1) >".into()),
            ("REVERB DSP SPU", "< ATIVADO (ECO 3D) >".into()),
            ("AMPLIFICACAO SPU", "< NORMAL (1.0x) >".into()),
        ],
        2 => vec![
            ("RENDERIZADOR", match config.render_mode {
                crate::gpu::RenderMode::Hardware => "< HARDWARE (GPU HD) >".into(),
                crate::gpu::RenderMode::Software => "< SOFTWARE (PS1) >".into(),
            }),
            ("OVERCLOCK CPU PS1", match cpu_clock_mode {
                0 => "< PADRAO (33.8 MHz) >".into(),
                1 => "< 1.5x (50.8 MHz) >".into(),
                _ => "< 2.0x (67.7 MHz) >".into(),
            }),
            ("SLOT SALVAMENTO", format!("< SLOT {} >", save_slot + 1)),
            ("ABRIR PASTA DE JOGOS", "< PRESSIONE A / ENTER >".into()),
            ("ABRIR PASTA DA BIOS", "< PRESSIONE A / ENTER >".into()),
        ],
        3 => vec![
            ("DISPOSITIVO ATIVO", if gamepad_connected { "XBOX PAD (XINPUT)".into() } else { "TECLADO USB".into() }),
            ("TIPO DE CONTROLE", "< DUALSHOCK ANALOGICO >".into()),
            ("VIBRACAO (RUMBLE)", match rumble_strength {
                0 => "< FORTE (DUPLO MOTOR) >".into(),
                1 => "< SUAVE >".into(),
                _ => "< DESATIVADA >".into(),
            }),
            ("DEADZONE ANALOGICO", "< 8000 (PADRAO) >".into()),
        ],
        _ => vec![],
    };

    let start_y = my + 34;
    for (i, (label, val)) in items.iter().enumerate() {
        let iy = start_y + i * 25;
        let is_sel = i == row_cursor;

        if is_sel {
            draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx + 10, iy - 3, mw - 20, 20, 0x16243D);
            draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx + 10, iy - 3, 3, 20, 0x00E5FF);
        }

        let l_color = if is_sel { 0xFFFFFF } else { 0x90A4AE };
        let v_color = if is_sel { 0x00E5FF } else { 0xCFD8DC };

        draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, label, mx + 20, iy + 2, l_color);
        let val_x = mx + mw - 20 - (val.len() * 6);
        draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, val, val_x, iy + 2, v_color);
    }

    let foot_y = my + mh - 18;
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, foot_y - 5, mw, 1, 0x1A253A);

    if gamepad_connected {
        draw_centered_badge_row(
            buffer,
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            mx,
            mw,
            foot_y,
            18,
            &[
                ("LB/RB", 0x37474F, 0xFFFFFF, "ABAS"),
                ("< >", 0x37474F, 0xFFFFFF, "AJUSTAR"),
                ("A", 0x2E7D32, 0xFFFFFF, "ALTERAR"),
                ("B", 0xC62828, 0xFFFFFF, "FECHAR"),
            ],
        );
    } else {
        draw_text_centered(
            buffer,
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            mx,
            mw,
            foot_y + 2,
            "1-4: ABAS   CIMA/BAIXO: ITENS   < >/ENTER: ALTERAR   ESC: FECHAR",
            0x78909C,
        );
    }
}

// ============================================================================
// MODAL DE DETALHES DO JOGO (X NO CARROSSEL / DETALHES)
// ============================================================================
pub fn render_game_detail_card(
    buffer: &mut [u32],
    rom: &RomEntry,
    gamepad_connected: bool,
    card_cursor: usize,
) {
    for px in buffer.iter_mut() {
        let p = *px;
        let r = (((p >> 16) & 0xFF) * 3) / 10;
        let g = (((p >> 8) & 0xFF) * 3) / 10;
        let b = ((p & 0xFF) * 3) / 10;
        *px = (r << 16) | (g << 8) | b;
    }

    let mw = 360;
    let mh = 198;
    let mx = (CANVAS_WIDTH - mw) / 2;
    let my = (CANVAS_HEIGHT - mh) / 2;

    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx - 1, my - 1, mw + 2, mh + 2, 0x00E5FF);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, my, mw, mh, 0x0C121D);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, my, mw, 24, 0x162238);

    let title_short = if rom.clean_title.len() > 34 { format!("{}...", &rom.clean_title[..31]) } else { rom.clean_title.clone() };
    draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, mw, my + 9, &title_short, 0xFFFFFF);

    let thumb_w = 70;
    let thumb_h = 100;
    let thumb_x = mx + 20;
    let thumb_y = my + 36;

    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, thumb_x - 1, thumb_y - 1, thumb_w + 2, thumb_h + 2, 0x00E5FF);
    for y in 0..thumb_h {
        for x in 0..thumb_w {
            let sx = (x * CARD_WIDTH) / thumb_w;
            let sy = (y * CARD_HEIGHT) / thumb_h;
            buffer[(thumb_y + y) * CANVAS_WIDTH + (thumb_x + x)] = rom.preview[sy * CARD_WIDTH + sx];
        }
    }

    let stat_x = mx + 106;
    let mut stat_y = my + 44;

    draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, "ESTATISTICAS DE JOGO", stat_x, stat_y, 0x00E5FF);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, stat_x, stat_y + 11, 230, 1, 0x1E2B45);
    stat_y += 22;

    let hours = rom.stats.play_time_seconds / 3600;
    let minutes = (rom.stats.play_time_seconds % 3600) / 60;
    let time_str = format!("TEMPO JOGADO: {}h {}min", hours, minutes);
    draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, &time_str, stat_x, stat_y, 0xFFFFFF);
    stat_y += 20;

    let sessions_str = format!("VEZES INICIADO: {}x", rom.stats.play_count);
    draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, &sessions_str, stat_x, stat_y, 0xCFD8DC);
    stat_y += 20;

    let fav_str = if rom.stats.is_favorite { "STATUS: FAVORITO NA BIBLIOTECA" } else { "STATUS: NA BIBLIOTECA" };
    let fav_col = if rom.stats.is_favorite { 0xFFD54F } else { 0x78909C };
    draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, fav_str, stat_x, stat_y, fav_col);

    let btn_y = my + 146;
    let bw = 96;
    let btn_gap = 16;
    let start_bx = mx + (mw - (bw * 3 + btn_gap * 2)) / 2;

    let btns = ["JOGAR", "RESETAR", "VOLTAR"];
    for (idx, &opt) in btns.iter().enumerate() {
        let bx = start_bx + idx * (bw + btn_gap);
        let is_sel = idx == card_cursor;

        let bg = if is_sel { 0x00E5FF } else { 0x141E30 };
        let text_c = if is_sel { 0x000000 } else { 0xCFD8DC };

        draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, bx, btn_y, bw, 22, bg);
        draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, bx, bw, btn_y + 8, opt, text_c);
    }

    let hint_y = my + mh - 18;
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, hint_y - 5, mw, 1, 0x1A253A);
    if gamepad_connected {
        draw_centered_badge_row(
            buffer,
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            mx,
            mw,
            hint_y,
            18,
            &[
                ("< >", 0x37474F, 0xFFFFFF, "AJUSTAR"),
                ("A", 0x2E7D32, 0xFFFFFF, "CONFIRMAR"),
                ("B", 0xC62828, 0xFFFFFF, "FECHAR"),
            ],
        );
    } else {
        draw_text_centered(
            buffer,
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            mx,
            mw,
            hint_y + 2,
            "< >: ESCOLHER   ENTER: CONFIRMAR   ESC: FECHAR",
            0x78909C,
        );
    }
}

// ============================================================================
// MENU IN-GAME SUSPENSO (ABERTO AO SEGURAR START NO JOGO)
// ============================================================================
pub fn render_in_game_menu(
    buffer: &mut [u32],
    game_title: &str,
    has_save: bool,
    active_tab: usize,
    row_cursor: usize,
    gamepad_connected: bool,
    config: &crate::gpu::GpuConfig,
    display_mode: DisplayMode,
    aspect_mode: AspectMode,
    save_slot: usize,
    audio_vol: u32,
) {
    for px in buffer.iter_mut() {
        let p = *px;
        let r = (((p >> 16) & 0xFF) * 2) / 10;
        let g = (((p >> 8) & 0xFF) * 2) / 10;
        let b = ((p & 0xFF) * 2) / 10;
        *px = (r << 16) | (g << 8) | b;
    }

    let mw = 420;
    let mh = 236;
    let mx = (CANVAS_WIDTH - mw) / 2;
    let my = (CANVAS_HEIGHT - mh) / 2;

    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx - 1, my - 1, mw + 2, mh + 2, 0x00E5FF);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, my, mw, mh, 0x0B0F19);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, my, mw, 26, 0x131A29);
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, my + 26, mw, 1, 0x1E2B42);

    let tabs = ["1.PARTIDA", "2.VIDEO", "3.AUDIO"];
    let tab_w = mw / tabs.len();

    for (t_idx, &t_name) in tabs.iter().enumerate() {
        let tx = mx + t_idx * tab_w;
        let is_tab_sel = t_idx == active_tab;

        if is_tab_sel {
            draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, tx, my + 1, tab_w, 25, 0x1D2A42);
            draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, tx, my + 24, tab_w, 2, 0x00E5FF);
        }
        let tc = if is_tab_sel { 0x00E5FF } else { 0x607D8B };
        draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, tx, tab_w, my + 10, t_name, tc);
    }

    let title_clean = if game_title.len() > 38 { &game_title[..38] } else { game_title };
    draw_text_centered(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, mw, my + 30, title_clean, 0x00E5FF);

    let items: Vec<(&str, String)> = match active_tab {
        0 => vec![
            ("CONTINUAR JOGANDO", "".into()),
            (match save_slot {
                0 => "SALVAR ESTADO [SLOT 1]",
                1 => "SALVAR ESTADO [SLOT 2]",
                2 => "SALVAR ESTADO [SLOT 3]",
                3 => "SALVAR ESTADO [SLOT 4]",
                _ => "SALVAR ESTADO [SLOT 5]",
            }, "[F2]".into()),
            (match save_slot {
                0 => "CARREGAR ESTADO [SLOT 1]",
                1 => "CARREGAR ESTADO [SLOT 2]",
                2 => "CARREGAR ESTADO [SLOT 3]",
                3 => "CARREGAR ESTADO [SLOT 4]",
                _ => "CARREGAR ESTADO [SLOT 5]",
            }, if has_save { "< PRONTO > [F4]".into() } else { "< VAZIO >".into() }),
            ("TROCAR SLOT ATIVO", format!("< SLOT {} >", save_slot + 1)),
            ("REINICIAR CONSOLE", "< RESET >".into()),
            ("SAIR PARA O HUB ARCADE", "".into()),
        ],
        1 => vec![
            ("MODO DE TELA", match display_mode {
                DisplayMode::Windowed => "< JANELA (1024x576) >".into(),
                DisplayMode::Borderless => "< SEM BORDAS >".into(),
                DisplayMode::Fullscreen => "< TELA CHEIA >".into(),
            }),
            ("FORMATO DE TELA", match aspect_mode {
                AspectMode::Classic4x3 => "< 4:3 ORIGINAL (BORDAS) >".into(),
                AspectMode::WidescreenHack => "< 16:9 HACK (FOV 3D REAL) >".into(),
                AspectMode::WidescreenStretch => "< 16:9 ESTICADO (FULL) >".into(),
            }),
            ("RESOLUCAO INTERNA", match config.internal_res {
                crate::gpu::InternalResolution::Native1x => "< 1x NATIVA (240p) >".into(),
                crate::gpu::InternalResolution::Hd2x => "< 2x HD (480p) >".into(),
                crate::gpu::InternalResolution::FullHd4x => "< 4x FULL HD (960p) >".into(),
            }),
            ("PRECISAO PGXP (SUBPIXEL)", if config.pgxp_enabled { "< ATIVADO >".into() } else { "< DESATIVADO >".into() }),
            ("SUAVIZACAO TEXTURA", if config.bilinear_filter { "< BILINEAR >".into() } else { "< NITIDO >".into() }),
            ("EFEITO DE TELA (CRT)", match config.shader {
                crate::gpu::ShaderEffect::None => "< DESATIVADO >".into(),
                crate::gpu::ShaderEffect::Bilinear => "< BILINEAR >".into(),
                crate::gpu::ShaderEffect::CrtScanlines => "< CRT SCANLINES >".into(),
                crate::gpu::ShaderEffect::BilinearCrt => "< BILINEAR + CRT >".into(),
                crate::gpu::ShaderEffect::RfNoise => "< TV ANTIGA RF >".into(),
            }),
        ],
        2 => vec![
            ("VOLUME GERAL", format!("< {}% >", audio_vol)),
            ("REVERB DSP SPU", "< ATIVADO >".into()),
        ],
        _ => vec![],
    };

    let start_y = my + 46;
    for (i, (label, val)) in items.iter().enumerate() {
        let iy = start_y + i * 25;
        let is_sel = i == row_cursor;

        if is_sel {
            draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx + 10, iy - 3, mw - 20, 20, 0x16243D);
            draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx + 10, iy - 3, 3, 20, 0x00E5FF);
        }

        let l_color = if is_sel { 0xFFFFFF } else { 0x90A4AE };
        let v_color = if is_sel { 0x00E5FF } else { 0xCFD8DC };

        draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, label, mx + 20, iy + 2, l_color);
        if !val.is_empty() {
            let val_x = mx + mw - 20 - (val.len() * 6);
            draw_text(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, &val, val_x, iy + 2, v_color);
        }
    }

    let foot_y = my + mh - 18;
    draw_rect(buffer, CANVAS_WIDTH, CANVAS_HEIGHT, mx, foot_y - 5, mw, 1, 0x1A253A);

    if gamepad_connected {
        draw_centered_badge_row(
            buffer,
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            mx,
            mw,
            foot_y,
            18,
            &[
                ("LB/RB", 0x37474F, 0xFFFFFF, "ABAS"),
                ("< >", 0x37474F, 0xFFFFFF, "AJUSTAR"),
                ("A", 0x2E7D32, 0xFFFFFF, "CONFIRMAR"),
                ("B", 0xC62828, 0xFFFFFF, "VOLTAR"),
            ],
        );
    } else {
        draw_text_centered(
            buffer,
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            mx,
            mw,
            foot_y + 2,
            "1-3: ABAS   CIMA/BAIXO: ITENS   ENTER: CONFIRMAR   ESC: VOLTAR",
            0x78909C,
        );
    }
}
