# 🎮 PEnginePs1 — Emulador de PlayStation 1 (PSX)

[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux-lightgrey.svg)]()
[![FPS](https://img.shields.io/badge/Performance-60.0%20FPS%20Locked-brightgreen.svg)]()

Um emulador de **PlayStation 1 (PSX)** moderno, ultraleve e de alta performance desenvolvido em **Rust**. Apresenta interface Arcade com carrossel 3D, download automático de capas oficiais, suporte completo a controles de **Xbox (XInput)** e teclado, áudio estéreo com reverb por hardware e suporte a **Widescreen 16:9 Anamórfico Real (sem esticar)**.

---

## ✨ Principais Funcionalidades

* **Interface Arcade Intuitiva:** Biblioteca com carrossel de capas 3D, busca dinâmica instantânea, filtros A-Z e histórico de jogo.
* **Download Automático de Capas:** Baixa e armazena em cache capas oficiais do banco de dados Libretro em segundo plano.
* **60 FPS Cravados:** Temporização multimídia precisa (`timeBeginPeriod(1)`), rasterização convexa antecipada e barramento em ponteiro sem overhead.
* **Hack Widescreen 16:9 Real:** Geometria 3D expandida via transformação anamórfica de FOV (0.75 x 1.333 = 1.0), sem achatar nem esticar personagens.
* **PGXP Subpixel Precision:** Estabiliza vértices e elimina a clássica tremedeira poligonal do PS1 com lookup $O(1)$ de alta velocidade.
* **Renderização HD & Shaders:**
  * Resoluções internas: 1x Nativa (240p), 2x HD (480p) e 4x Full HD (960p).
  * Shaders de vídeo: Pixel Art puro, Bilinear, CRT Scanlines e TV Antiga RF com chiado.
* **Áudio Estéreo SPU de Alta Fidelidade:** 24 vozes ADSR, decodificação SPU-ADPCM, interpolação gaussiana e processador de eco/reverb por hardware.
* **Salvar & Carregar Estados (Save State):** 5 slots com prévia rápida, atalhos de teclado e joystick.
* **Menu In-Game Instantâneo:** Segure START ou aperte ESC dentro do jogo para pausar, salvar/carregar e ajustar opções sem sair da partida.
* **Build Sem Terminal:** Inicia diretamente como aplicativo nativo limpo sem janela preta de prompt.

---

## 🕹️ Controles

| Ação | Controle de Xbox (XInput) | Teclado USB |
| :--- | :--- | :--- |
| **Navegar nos Jogos** | D-Pad / Analógico Esquerdo | Setas (Cima / Baixo / Esq / Dir) |
| **Abrir Jogo / Confirmar** | Botão A | Enter / Z |
| **Voltar / Cancelar** | Botão B | ESC / X |
| **Busca de Jogos** | Botão X | F2 |
| **Abrir Pasta de Jogos** | Botão Y | F1 |
| **Alternar Categoria / Filtro** | Gatilho LT | Tab |
| **Menu de Configurações (Hub)**| START | F3 / M |
| **Menu In-Game (durante o jogo)** | Segurar START | ESC |
| **Salvar Estado Rápido** | — | F5 |
| **Carregar Estado Rápido** | — | F8 |
| **Modo Janela / Tela Cheia** | — | F11 / Alt + Enter |
| **Monitor de Hardware (DEV)** | — | F12 (ou `--dev`) |

---

## 🚀 Como Compilar e Executar

### Pré-requisitos
* [Rust e Cargo](https://rustup.rs/) (versão estável recomendada)

### Compilação (Release)
```bash
cargo build --release
```
O executável compilado estará localizado em:
```text
target/release/PEnginePs1.exe
```

### Onde Colocar os Jogos e a BIOS
O emulador cria automaticamente pastas organizadas em **Documentos**:
* `Documentos\PEnginePs1\bios` (ou pasta local `bios/`): Coloque a BIOS (ex: `SCPH1001.BIN`).
* `Documentos\PEnginePs1\isos` (ou pasta local `isos/`): Coloque seus jogos (`.cue`, `.bin`, `.iso`, `.chd`, `.pbp`).

Você também pode abrir essas pastas a qualquer momento pelo menu de opções do próprio emulador!

---

## 🛠️ Arquitetura do Sistema

* `src/cpu.rs`: Emulação completa do MIPS R3000A com pipeline de load delay e I-Cache.
* `src/gte.rs`: Geometry Transform Engine (Coprocessador 2) para matemática 3D e perspectiva.
* `src/gpu.rs`: GPU do PS1 com suporte a modos 15/24-bit, dithering, PGXP e rasterização de triângulos convexos.
* `src/spu.rs`: Processador de som de 24 canais, ADSR, decodificação ADPCM e DSP de Reverb.
* `src/cdrom.rs`: Controlador de drive de CD-ROM com comandos assíncronos e leitura de setores.
* `src/dma.rs`: Controlador de DMA de 7 canais (GPU, SPU, CD-ROM, MDEC, OTC).
* `src/mdec.rs`: Decodificador Macroblock para vídeos FMV e descompressão DCT.
* `src/hub.rs`: Interface gráfica Arcade com renderização de carrossel, capas e menus.
* `src/pgxp.rs`: Cache associativo de 4 vias $O(1)$ para subpixel precision.

---

## 📄 Licença

Distribuído sob a licença MIT. Consulte `LICENSE` para mais detalhes.
