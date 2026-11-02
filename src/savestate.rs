// src/savestate.rs
// Sistema de Salvamento e Carregamento Rápido de Estado (Quick Save / Quick Load)
// Grava e restaura o snapshot completo da CPU, RAM (2MB), VRAM (1MB), SPU (512KB),
// Timers, DMA e Registradores em ~2 milissegundos.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use crate::cpu::Cpu;
use crate::bus::Bus;

const SAVE_MAGIC: &[u8; 8] = b"PSXSAVE1";

pub fn save_state(file_path: &str, cpu: &Cpu, bus: &Bus) -> Result<(), String> {
    let mut data = Vec::with_capacity(3_800_000);

    // 1. Magic Header
    data.extend_from_slice(SAVE_MAGIC);

    // 2. CPU
    data.extend_from_slice(&cpu.pc.to_le_bytes());
    data.extend_from_slice(&cpu.next_pc.to_le_bytes());
    data.extend_from_slice(&cpu.current_pc.to_le_bytes());
    for reg in &cpu.regs {
        data.extend_from_slice(&reg.to_le_bytes());
    }
    data.extend_from_slice(&cpu.hi.to_le_bytes());
    data.extend_from_slice(&cpu.lo.to_le_bytes());
    data.extend_from_slice(&cpu.bpc.to_le_bytes());
    data.extend_from_slice(&cpu.bda.to_le_bytes());
    data.extend_from_slice(&cpu.jumpdest.to_le_bytes());
    data.extend_from_slice(&cpu.dcic.to_le_bytes());
    data.extend_from_slice(&cpu.bad_vaddr.to_le_bytes());
    data.extend_from_slice(&cpu.bdam.to_le_bytes());
    data.extend_from_slice(&cpu.bpcm.to_le_bytes());
    data.extend_from_slice(&cpu.sr.to_le_bytes());
    data.extend_from_slice(&cpu.cause.to_le_bytes());
    data.extend_from_slice(&cpu.epc.to_le_bytes());

    // 3. RAM (2MB) e Scratchpad (1KB)
    data.extend_from_slice(&bus.ram[..]);
    data.extend_from_slice(&bus.scratchpad[..]);

    // 4. GPU (VRAM 1024x512 = 1MB) e Registradores
    for px in bus.gpu.vram.iter() {
        data.extend_from_slice(&px.to_le_bytes());
    }
    data.extend_from_slice(&bus.gpu.status.to_le_bytes());
    data.extend_from_slice(&bus.gpu.draw_area_left.to_le_bytes());
    data.extend_from_slice(&bus.gpu.draw_area_top.to_le_bytes());
    data.extend_from_slice(&bus.gpu.draw_area_right.to_le_bytes());
    data.extend_from_slice(&bus.gpu.draw_area_bottom.to_le_bytes());
    data.extend_from_slice(&bus.gpu.draw_offset_x.to_le_bytes());
    data.extend_from_slice(&bus.gpu.draw_offset_y.to_le_bytes());
    data.extend_from_slice(&(bus.gpu.page_base_x as u32).to_le_bytes());
    data.extend_from_slice(&(bus.gpu.page_base_y as u32).to_le_bytes());
    data.push(bus.gpu.semi_transparency_mode);
    data.push(bus.gpu.texture_depth);
    data.push(if bus.gpu.dither { 1 } else { 0 });
    data.push(if bus.gpu.draw_to_display { 1 } else { 0 });
    data.push(if bus.gpu.texture_disable { 1 } else { 0 });
    data.extend_from_slice(&(bus.gpu.clut_x as u32).to_le_bytes());
    data.extend_from_slice(&(bus.gpu.clut_y as u32).to_le_bytes());
    data.extend_from_slice(&(bus.gpu.display_vram_x as u32).to_le_bytes());
    data.extend_from_slice(&(bus.gpu.display_vram_y as u32).to_le_bytes());
    data.push(if bus.gpu.display_depth_24 { 1 } else { 0 });

    // 5. SPU (RAM 512KB) e Registradores
    for word in bus.spu.ram.iter() {
        data.extend_from_slice(&word.to_le_bytes());
    }
    data.extend_from_slice(&bus.spu.control.to_le_bytes());
    data.extend_from_slice(&bus.spu.transfer_control.to_le_bytes());
    data.extend_from_slice(&bus.spu.main_vol_left.to_le_bytes());
    data.extend_from_slice(&bus.spu.main_vol_right.to_le_bytes());
    for v in &bus.spu.voices {
        data.extend_from_slice(&v.vol_left.to_le_bytes());
        data.extend_from_slice(&v.vol_right.to_le_bytes());
        data.extend_from_slice(&v.pitch.to_le_bytes());
        data.extend_from_slice(&v.start_addr.to_le_bytes());
        data.extend_from_slice(&v.adsr_lo.to_le_bytes());
        data.extend_from_slice(&v.adsr_hi.to_le_bytes());
        data.extend_from_slice(&v.adsr_vol.to_le_bytes());
        data.extend_from_slice(&v.repeat_addr.to_le_bytes());
        data.extend_from_slice(&v.cur_addr.to_le_bytes());
    }

    // 6. DMA, Timers e IRQ
    data.extend_from_slice(&bus.dma.control.to_le_bytes());
    data.extend_from_slice(&bus.dma.interrupt.to_le_bytes());
    for ch in &bus.dma.channels {
        data.extend_from_slice(&ch.base_addr.to_le_bytes());
        data.extend_from_slice(&ch.block_ctrl.to_le_bytes());
        data.extend_from_slice(&ch.ctrl.to_le_bytes());
    }
    for t in &bus.timers.timers {
        data.extend_from_slice(&t.counter.to_le_bytes());
        data.extend_from_slice(&t.mode.to_le_bytes());
        data.extend_from_slice(&t.target.to_le_bytes());
        data.extend_from_slice(&t.sub_cycles.to_le_bytes());
    }
    data.extend_from_slice(&bus.irq.status.to_le_bytes());
    data.extend_from_slice(&bus.irq.mask.to_le_bytes());

    let mut f = File::create(file_path).map_err(|e| format!("Erro ao criar arquivo de save state: {}", e))?;
    f.write_all(&data).map_err(|e| format!("Erro ao gravar dados do save state: {}", e))?;
    Ok(())
}

pub fn load_state(file_path: &str, cpu: &mut Cpu, bus: &mut Bus) -> Result<(), String> {
    if !Path::new(file_path).exists() {
        return Err("Arquivo de save state nao encontrado!".to_string());
    }

    let mut f = File::open(file_path).map_err(|e| format!("Erro ao abrir save state: {}", e))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data).map_err(|e| format!("Erro ao ler save state: {}", e))?;

    if data.len() < 8 || &data[0..8] != SAVE_MAGIC {
        return Err("Arquivo de save state invalido ou corrompido!".to_string());
    }

    let mut pos = 8;

    let read_u32 = |p: &mut usize| -> u32 {
        let val = u32::from_le_bytes(data[*p..*p + 4].try_into().unwrap());
        *p += 4;
        val
    };
    let read_i32 = |p: &mut usize| -> i32 {
        let val = i32::from_le_bytes(data[*p..*p + 4].try_into().unwrap());
        *p += 4;
        val
    };
    let read_u16 = |p: &mut usize| -> u16 {
        let val = u16::from_le_bytes(data[*p..*p + 2].try_into().unwrap());
        *p += 2;
        val
    };

    // 2. CPU
    cpu.pc = read_u32(&mut pos);
    cpu.next_pc = read_u32(&mut pos);
    cpu.current_pc = read_u32(&mut pos);
    for reg in &mut cpu.regs {
        *reg = read_u32(&mut pos);
    }
    cpu.hi = read_u32(&mut pos);
    cpu.lo = read_u32(&mut pos);
    cpu.bpc = read_u32(&mut pos);
    cpu.bda = read_u32(&mut pos);
    cpu.jumpdest = read_u32(&mut pos);
    cpu.dcic = read_u32(&mut pos);
    cpu.bad_vaddr = read_u32(&mut pos);
    cpu.bdam = read_u32(&mut pos);
    cpu.bpcm = read_u32(&mut pos);
    cpu.sr = read_u32(&mut pos);
    cpu.cause = read_u32(&mut pos);
    cpu.epc = read_u32(&mut pos);

    // 3. RAM e Scratchpad
    bus.ram.copy_from_slice(&data[pos..pos + crate::bus::RAM_SIZE]);
    pos += crate::bus::RAM_SIZE;
    bus.scratchpad.copy_from_slice(&data[pos..pos + crate::bus::SCRATCHPAD_SIZE]);
    pos += crate::bus::SCRATCHPAD_SIZE;

    // 4. GPU VRAM e Registradores
    for px in bus.gpu.vram.iter_mut() {
        *px = read_u16(&mut pos);
    }
    bus.gpu.status = read_u32(&mut pos);
    bus.gpu.draw_area_left = read_i32(&mut pos);
    bus.gpu.draw_area_top = read_i32(&mut pos);
    bus.gpu.draw_area_right = read_i32(&mut pos);
    bus.gpu.draw_area_bottom = read_i32(&mut pos);
    bus.gpu.draw_offset_x = read_i32(&mut pos);
    bus.gpu.draw_offset_y = read_i32(&mut pos);
    bus.gpu.page_base_x = read_u32(&mut pos) as usize;
    bus.gpu.page_base_y = read_u32(&mut pos) as usize;
    bus.gpu.semi_transparency_mode = data[pos]; pos += 1;
    bus.gpu.texture_depth = data[pos]; pos += 1;
    bus.gpu.dither = data[pos] != 0; pos += 1;
    bus.gpu.draw_to_display = data[pos] != 0; pos += 1;
    bus.gpu.texture_disable = data[pos] != 0; pos += 1;
    bus.gpu.clut_x = read_u32(&mut pos) as usize;
    bus.gpu.clut_y = read_u32(&mut pos) as usize;
    bus.gpu.display_vram_x = read_u32(&mut pos) as usize;
    bus.gpu.display_vram_y = read_u32(&mut pos) as usize;
    bus.gpu.display_depth_24 = data[pos] != 0; pos += 1;

    // 5. SPU
    for word in bus.spu.ram.iter_mut() {
        *word = read_u16(&mut pos);
    }
    bus.spu.control = read_u16(&mut pos);
    bus.spu.transfer_control = read_u16(&mut pos);
    bus.spu.main_vol_left = read_u16(&mut pos);
    bus.spu.main_vol_right = read_u16(&mut pos);
    for v in &mut bus.spu.voices {
        v.vol_left = read_u16(&mut pos);
        v.vol_right = read_u16(&mut pos);
        v.pitch = read_u16(&mut pos);
        v.start_addr = read_u16(&mut pos);
        v.adsr_lo = read_u16(&mut pos);
        v.adsr_hi = read_u16(&mut pos);
        v.adsr_vol = read_i32(&mut pos);
        v.repeat_addr = read_u16(&mut pos);
        v.cur_addr = read_u32(&mut pos);
        v.block_decoded = false;
    }

    // 6. DMA, Timers e IRQ
    bus.dma.control = read_u32(&mut pos);
    bus.dma.interrupt = read_u32(&mut pos);
    for ch in &mut bus.dma.channels {
        ch.base_addr = read_u32(&mut pos);
        ch.block_ctrl = read_u32(&mut pos);
        ch.ctrl = read_u32(&mut pos);
    }
    for t in &mut bus.timers.timers {
        t.counter = read_u16(&mut pos);
        t.mode = read_u16(&mut pos);
        t.target = read_u16(&mut pos);
        t.sub_cycles = read_u32(&mut pos);
    }
    bus.irq.status = read_u16(&mut pos);
    bus.irq.mask = read_u16(&mut pos);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_load_state() {
        let bios_bytes = vec![0u8; 512 * 1024];
        let mut bus = Bus::new(bios_bytes);
        let mut cpu = Cpu::new();

        cpu.regs[1] = 0xCAFEBABE;
        cpu.pc = 0x80054320;
        bus.ram[0x1000] = 0x42;
        bus.gpu.vram[500] = 0x7FFF;

        let tmp_file = "test_snapshot.state";
        let res = save_state(tmp_file, &cpu, &bus);
        assert!(res.is_ok());

        cpu.regs[1] = 0;
        cpu.pc = 0;
        bus.ram[0x1000] = 0;
        bus.gpu.vram[500] = 0;

        let load_res = load_state(tmp_file, &mut cpu, &mut bus);
        assert!(load_res.is_ok());

        assert_eq!(cpu.regs[1], 0xCAFEBABE);
        assert_eq!(cpu.pc, 0x80054320);
        assert_eq!(bus.ram[0x1000], 0x42);
        assert_eq!(bus.gpu.vram[500], 0x7FFF);

        let _ = std::fs::remove_file(tmp_file);
    }
}
