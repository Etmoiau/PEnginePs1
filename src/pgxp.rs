// src/pgxp.rs
// PGXP (Precision Geometry Transform Pipeline)
// Rastreamento de vértices em ponto flutuante de alta precisão entre o GTE e a GPU,
// eliminando a tremedeira poligonal (wobble) e permitindo correção de perspectiva.
//
// Otimizado com Cache Associativo de 4 Vias (4-way set associative) para lookup O(1)
// instantâneo sem loops lentos de busca linear.

#[derive(Clone, Copy, Debug)]
pub struct PgxpVertex {
    pub sx: i16,
    pub sy: i16,
    pub fx: f32,
    pub fy: f32,
    pub fw: f32,
    pub valid: bool,
}

impl Default for PgxpVertex {
    fn default() -> Self {
        Self {
            sx: 0,
            sy: 0,
            fx: 0.0,
            fy: 0.0,
            fw: 1.0,
            valid: false,
        }
    }
}

const PGXP_SETS: usize = 512;
const PGXP_WAYS: usize = 4;

struct PgxpCacheSet {
    ways: [PgxpVertex; PGXP_WAYS],
    replace_idx: usize,
}

static mut PGXP_CACHE: [PgxpCacheSet; PGXP_SETS] = [const {
    PgxpCacheSet {
        ways: [
            PgxpVertex { sx: 0, sy: 0, fx: 0.0, fy: 0.0, fw: 1.0, valid: false },
            PgxpVertex { sx: 0, sy: 0, fx: 0.0, fy: 0.0, fw: 1.0, valid: false },
            PgxpVertex { sx: 0, sy: 0, fx: 0.0, fy: 0.0, fw: 1.0, valid: false },
            PgxpVertex { sx: 0, sy: 0, fx: 0.0, fy: 0.0, fw: 1.0, valid: false },
        ],
        replace_idx: 0,
    }
}; PGXP_SETS];

#[inline(always)]
fn pgxp_hash(sx: i16, sy: i16) -> usize {
    let ux = (sx as u32) & 0x7FF;
    let uy = (sy as u32) & 0x7FF;
    ((ux ^ (uy << 4)) as usize) & (PGXP_SETS - 1)
}

/// Registra o vértice de alta precisão recém-projetado pelo GTE
#[inline(always)]
pub fn record_pgxp(sx: i16, sy: i16, fx: f32, fy: f32, fw: f32) {
    let set_idx = pgxp_hash(sx, sy);
    unsafe {
        let set = &mut PGXP_CACHE[set_idx];
        let way = set.replace_idx;
        set.ways[way] = PgxpVertex {
            sx,
            sy,
            fx,
            fy,
            fw,
            valid: true,
        };
        set.replace_idx = (way + 1) & (PGXP_WAYS - 1);
    }
}

/// Consulta o cache de PGXP para recuperar subpixels e profundidade 1/W em O(1)
#[inline(always)]
pub fn lookup_pgxp(sx: i16, sy: i16, off_x: i32, off_y: i32) -> (f32, f32, f32) {
    let set_idx = pgxp_hash(sx, sy);
    unsafe {
        let set = &PGXP_CACHE[set_idx];
        // Varre apenas as 4 vias do bucket (no máximo 4 iterações contra as antigas 512!)
        for v in &set.ways {
            if v.valid && v.sx == sx && v.sy == sy {
                return (v.fx + off_x as f32, v.fy + off_y as f32, v.fw);
            }
        }
    }
    // Fallback rápido: se for elemento 2D de interface ou não estiver no cache
    ((sx as i32 + off_x) as f32, (sy as i32 + off_y) as f32, 1.0)
}

/// Limpa o cache de PGXP (usado na troca de cena ou reset)
pub fn clear_pgxp_cache() {
    unsafe {
        for i in 0..PGXP_SETS {
            PGXP_CACHE[i].replace_idx = 0;
            for j in 0..PGXP_WAYS {
                PGXP_CACHE[i].ways[j].valid = false;
            }
        }
    }
}
