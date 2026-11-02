use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const CACHE_SECTORS: usize = 32; // Buffer ultraleve de ~75 KB (zero desperdício de RAM)

pub struct Disc {
    file: File,
    pub sector_size: usize,
    pub header_size: usize,
    cache_base_lba: u32,
    cache_num_sectors: usize,
    cache_buffer: Vec<u8>,
}

impl Disc {
    /// Abre uma imagem .cue, .bin ou .iso.
    pub fn open(path_str: &str) -> Result<Self, String> {
        let path = Path::new(path_str);
        let is_cue = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("cue"));
        let actual_path = if is_cue {
            let content = std::fs::read_to_string(path)
                .map_err(|e| format!("Erro ao ler .cue: {e}"))?;
            let bin_name = content
                .lines()
                .filter_map(|line| {
                    let trimmed = line.trim();
                    if !trimmed.to_ascii_uppercase().starts_with("FILE") {
                        return None;
                    }
                    let first_quote = trimmed.find('"')?;
                    let rest = &trimmed[first_quote + 1..];
                    let end_quote = rest.find('"')?;
                    Some(rest[..end_quote].to_string())
                })
                .next()
                .ok_or_else(|| "Não foi possível encontrar a linha FILE no .cue".to_string())?;
            path.parent().unwrap_or(Path::new("")).join(bin_name)
        } else {
            path.to_path_buf()
        };

        let mut file = File::open(&actual_path)
            .map_err(|e| format!("Erro ao abrir '{}': {e}", actual_path.display()))?;
        let mut sync_bytes = [0u8; 12];
        let is_raw = file.read_exact(&mut sync_bytes).is_ok()
            && sync_bytes[0] == 0
            && sync_bytes[11] == 0
            && sync_bytes[1..11].iter().all(|&b| b == 0xFF);
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;

        let (sector_size, header_size) = if is_raw { (2352, 24) } else { (2048, 0) };
        println!("Imagem de CD aberta: {}", actual_path.display());
        println!("Formato detectado : {} bytes/setor (payload: 2048 bytes)", sector_size);

        let cache_buffer = vec![0u8; sector_size * CACHE_SECTORS];
        Ok(Self {
            file,
            sector_size,
            header_size,
            cache_base_lba: u32::MAX,
            cache_num_sectors: 0,
            cache_buffer,
        })
    }

    /// Preenche o cache de setores a partir do LBA solicitado em uma única chamada ao SO.
    fn fill_cache(&mut self, lba: u32) -> Result<(), String> {
        let offset = (lba as u64) * (self.sector_size as u64);
        self.file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;

        let mut total_read = 0;
        let target_len = self.cache_buffer.len();
        while total_read < target_len {
            match self.file.read(&mut self.cache_buffer[total_read..]) {
                Ok(0) => break,
                Ok(n) => total_read += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.to_string()),
            }
        }

        let sectors_read = total_read / self.sector_size;
        if sectors_read == 0 {
            return Err("Fim do arquivo de imagem do CD atingido.".to_string());
        }
        self.cache_base_lba = lba;
        self.cache_num_sectors = sectors_read;
        Ok(())
    }

    /// Lê os 2048 bytes de dados de um setor LBA diretamente do cache em RAM.
    pub fn read_sector_data(&mut self, lba: u32) -> Result<[u8; 2048], String> {
        if lba < self.cache_base_lba || lba >= self.cache_base_lba.saturating_add(self.cache_num_sectors as u32) {
            self.fill_cache(lba)?;
        }

        let rel_sector = (lba - self.cache_base_lba) as usize;
        let start = rel_sector * self.sector_size + self.header_size;
        let mut buffer = [0u8; 2048];
        buffer.copy_from_slice(&self.cache_buffer[start..start + 2048]);
        Ok(buffer)
    }

    /// Lê um setor bruto para o DMA do CD-ROM diretamente do cache em RAM.
    pub fn read_sector_raw(&mut self, lba: u32, out: &mut [u8; 2352]) -> Result<(), String> {
        if lba < self.cache_base_lba || lba >= self.cache_base_lba.saturating_add(self.cache_num_sectors as u32) {
            self.fill_cache(lba)?;
        }

        let rel_sector = (lba - self.cache_base_lba) as usize;
        let start = rel_sector * self.sector_size;
        let end = start + self.sector_size;

        if self.sector_size == 2352 {
            out.copy_from_slice(&self.cache_buffer[start..end]);
        } else {
            out.fill(0);
            out[24..2072].copy_from_slice(&self.cache_buffer[start..end]);
        }
        Ok(())
    }

    /// Lê SYSTEM.CNF na ISO9660 e extrai o executável de boot indicado em BOOT.
    pub fn extract_boot_executable(&mut self) -> Result<Vec<u8>, String> {
        let pvd = self.read_sector_data(16)?;
        if &pvd[1..6] != b"CD001" {
            return Err("Identificador ISO9660 'CD001' não encontrado no setor 16!".into());
        }
        let root_lba = u32::from_le_bytes(pvd[158..162].try_into().unwrap());
        let root_size = u32::from_le_bytes(pvd[166..170].try_into().unwrap()) as usize;
        let mut root_data = self.read_file_bytes(root_lba, root_size)?;

        let cnf_entry = find_file_in_dir(&root_data, "SYSTEM.CNF")
            .ok_or_else(|| "Arquivo SYSTEM.CNF não encontrado no disco!".to_string())?;
        let cnf_bytes = self.read_file_bytes(cnf_entry.0, cnf_entry.1)?;
        let cnf_text = String::from_utf8_lossy(&cnf_bytes);
        let boot_name = cnf_text.lines().find_map(|line| {
            let clean = line.replace(' ', "");
            if !clean.to_ascii_uppercase().starts_with("BOOT=") {
                return None;
            }
            let file_part = clean.rsplit('\\').next()?;
            Some(file_part.split(';').next().unwrap_or(file_part).to_string())
        }).ok_or_else(|| "Não foi possível extrair a linha BOOT do SYSTEM.CNF".to_string())?;

        println!("Arquivo de Boot identificado: {}", boot_name);
        let entry = find_file_in_dir(&root_data, &boot_name)
            .ok_or_else(|| format!("Executável '{}' não encontrado no disco!", boot_name))?;
        root_data.clear();
        self.read_file_bytes(entry.0, entry.1)
    }

    fn read_file_bytes(&mut self, lba: u32, size: usize) -> Result<Vec<u8>, String> {
        let sectors = size.checked_add(2047)
            .ok_or_else(|| "Tamanho de arquivo ISO inválido.".to_string())? / 2048;
        let mut result = Vec::with_capacity(sectors.saturating_mul(2048));
        for i in 0..sectors {
            let sector_lba = lba.checked_add(i as u32)
                .ok_or_else(|| "LBA fora do intervalo suportado.".to_string())?;
            result.extend_from_slice(&self.read_sector_data(sector_lba)?);
        }
        result.truncate(size);
        Ok(result)
    }
}

fn find_file_in_dir(dir_data: &[u8], target_name: &str) -> Option<(u32, usize)> {
    let target = target_name.to_ascii_uppercase();
    let mut offset = 0usize;
    while offset < dir_data.len() {
        let entry_len = *dir_data.get(offset)? as usize;
        if entry_len == 0 {
            offset = ((offset / 2048) + 1) * 2048;
            continue;
        }
        let end = offset.checked_add(entry_len)?;
        if end > dir_data.len() || entry_len < 34 {
            return None;
        }
        let lba = u32::from_le_bytes(dir_data.get(offset + 2..offset + 6)?.try_into().ok()?);
        let size = u32::from_le_bytes(dir_data.get(offset + 10..offset + 14)?.try_into().ok()?) as usize;
        let name_len = *dir_data.get(offset + 32)? as usize;
        let name_end = (offset + 33).checked_add(name_len)?;
        if name_end <= end {
            let name = String::from_utf8_lossy(&dir_data[offset + 33..name_end]).to_ascii_uppercase();
            let base = name.split(';').next().unwrap_or(&name);
            if base == target || base.strip_suffix(".CNF") == Some(target.as_str()) {
                return Some((lba, size));
            }
        }
        offset = end;
    }
    None
}
