use crate::bus::{Bus, RAM_SIZE};
use crate::cpu::Cpu;
use std::fs;

pub struct ExeLoader;

impl ExeLoader {
    pub fn load_and_boot(path: &str, cpu: &mut Cpu, bus: &mut Bus) -> Result<(), String> {
        let bytes = fs::read(path).map_err(|e| format!("Erro ao abrir '{}': {}", path, e))?;
        Self::load_from_bytes(&bytes, cpu, bus)
    }

    pub fn load_from_bytes(bytes: &[u8], cpu: &mut Cpu, bus: &mut Bus) -> Result<(), String> {
        const HEADER_SIZE: usize = 2048;
        if bytes.len() < HEADER_SIZE {
            return Err("Arquivo muito pequeno para ser um PS-X EXE!".into());
        }
        if &bytes[0..8] != b"PS-X EXE" {
            return Err("Assinatura inválida! Não é um executável PS-X.".into());
        }

        let read_u32 = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let initial_pc = read_u32(0x10);
        let initial_gp = read_u32(0x14);
        let dest_ram_addr = read_u32(0x18);
        let file_size = read_u32(0x1C) as usize;
        let sp_base = read_u32(0x30);
        let sp_offset = read_u32(0x34);
        let initial_sp = sp_base.wrapping_add(sp_offset);

        let payload_end = HEADER_SIZE
            .checked_add(file_size)
            .ok_or_else(|| "Tamanho do executável excede os limites suportados.".to_string())?;
        if payload_end > bytes.len() {
            return Err(format!(
                "Executável truncado: cabeçalho declara {} bytes, mas só há {} bytes de payload.",
                file_size,
                bytes.len().saturating_sub(HEADER_SIZE)
            ));
        }

        let physical_addr = Bus::physical_address(dest_ram_addr);
        let ram_offset = (physical_addr & 0x001F_FFFF) as usize;
        let ram_end = ram_offset
            .checked_add(file_size)
            .ok_or_else(|| "Endereço de destino excede os limites da RAM.".to_string())?;
        if physical_addr >= RAM_SIZE as u32 || ram_end > RAM_SIZE {
            return Err(format!(
                "Destino do executável (0x{dest_ram_addr:08X}) não cabe na RAM de 2 MB."
            ));
        }

        println!("--- Executando Binário do Jogo ---");
        println!("Destino RAM : 0x{:08X}", dest_ram_addr);
        println!("Tamanho     : {} bytes", file_size);
        println!("PC Inicial  : 0x{:08X}", initial_pc);
        println!("SP Inicial  : 0x{:08X}", initial_sp);
        println!("Ponteiro GP : 0x{:08X}", initial_gp);
        println!("----------------------------------");

        bus.ram[ram_offset..ram_end].copy_from_slice(&bytes[HEADER_SIZE..payload_end]);

        cpu.pc = initial_pc;
        cpu.next_pc = initial_pc.wrapping_add(4);
        cpu.current_pc = initial_pc;
        if initial_sp != 0 {
            cpu.set_reg(29, initial_sp);
            cpu.set_reg(30, initial_sp);
        }
        if initial_gp != 0 {
            cpu.set_reg(28, initial_gp);
        }

        println!("Executável carregado na RAM com sucesso!");
        Ok(())
    }
}
