# PEnginePs1

Emulador de PlayStation 1 (PSX) escrito em Rust, com interface visual para seleção de jogos, suporte a controle de Xbox e teclado, precisão de vértices PGXP, áudio SPU com reverberação e opções de vídeo.

> **Importante sobre ISOs e BIOS:** este repositório não contém nenhum arquivo de jogo (ISO/BIN/CUE/CHD) e nenhuma imagem de BIOS do PlayStation. O PEnginePs1 também não baixa nem distribui esses arquivos. Para jogar, cada pessoa precisa fornecer seus próprios arquivos de jogos e sua própria BIOS obtida legalmente, sendo responsável por ter o direito de usá-los de acordo com as leis aplicáveis em seu local.

## Estado do projeto e compatibilidade

Este emulador foi desenvolvido primordialmente para estudo, experimentação e uso pessoal. O projeto não está 100% completo nem oferece compatibilidade total com toda a biblioteca de títulos do PlayStation 1. Certos jogos podem apresentar falhas visuais, problemas de temporização, áudio imperfeito ou travamentos. O código continua sendo aprimorado de forma contínua.

## Recursos

- Biblioteca visual com capas de jogos, histórico recente e busca dinâmica.
- Busca por título, navegação alfabética e filtro por categorias.
- Suporte a controle de Xbox compatível com XInput e teclado.
- Modos de vídeo com filtros Bilinear, CRT Scanlines, TV antiga e Pixel Art nativo.
- Renderização interna nas resoluções 1x (nativa), 2x (HD) e 4x (Full HD).
- Correção de perspectiva e estabilização de geometria poligonal via PGXP.
- Suporte a proporção widescreen 16:9 anamórfica sem esticar a imagem.
- Processador de áudio SPU com 24 vozes, decodificação ADPCM e efeito de reverberação por hardware.
- Salvamento e carregamento de estados (Save States) com 5 slots disponíveis.
- Menu de pausa integrado para ajustes em tempo real sem fechar o jogo.
- Execução como aplicativo nativo do Windows, sem janela de terminal.

## Jogos e BIOS: use seus próprios arquivos

Nenhum jogo ou BIOS acompanha o PEnginePs1. Não inclua arquivos proprietários no repositório.

No Windows, o emulador cria e busca arquivos automaticamente nestas pastas:

- Pasta de jogos (ISOs):
```text
%USERPROFILE%\Documents\meu_psx\isos
```
(ou na pasta local `isos` ao lado do executável)

- Pasta de BIOS:
```text
%USERPROFILE%\Documents\meu_psx\bios
```
(ou na pasta local `bios` ao lado do executável)

Coloque suas imagens de jogos (`.cue` acompanhado de `.bin`, ou `.iso`, `.img`, `.chd`) na pasta de jogos, e a BIOS do PlayStation (como `SCPH1001.BIN`, `scph5501.bin`, etc.) na pasta de BIOS. É possível pressionar **F1** no Hub para escolher outra pasta de jogos ou acessar a opção no menu de configurações (F3 / Start).

## Requisitos

- Windows 10 ou posterior de 64 bits.
- Rust e Cargo instalados pelo [rustup](https://rustup.rs/).
- Dispositivo de som compatível para saída de áudio estéreo a 44.1 kHz.

## Compilar e executar

Abra um terminal na pasta do projeto e execute:

```powershell
cargo run --release
```

Para apenas compilar:

```powershell
cargo build --release
```

O executável compilado ficará em `target\release\PEnginePs1.exe`.

## Controles

### Hub

| Ação | Teclado | Gamepad Xbox |
|---|---|---|
| Navegar pela biblioteca | Setas | Direcional ou analógico esquerdo |
| Abrir / Confirmar | Enter ou Z | A |
| Voltar | Esc | B |
| Buscar | F2 ou X | X |
| Escolher pasta de jogos | F1 ou Y | Y |
| Abrir configurações | F3 ou M | Menu / Start |
| Trocar categoria | Tab | LT |
| Saltar entre letras | Page Up / Page Down | LB / RB |

### Durante o jogo

| Ação PS1 | Teclado | Gamepad Xbox |
|---|---|---|
| Cruz (X) | Z | A |
| Círculo (O) | X | B |
| Quadrado (□) | A | X |
| Triângulo (△) | S | Y |
| Direcional | Setas | Direcional ou analógico esquerdo |
| L1 / R1 | Q / E | LB / RB |
| L2 / R2 | 1 / 3 | LT / RT |
| Select / Start | Espaço / Enter | Back / Start |
| Pausar e abrir menu | Esc | Segurar Start |
| Alternar modo analógico | F5 | — |
| Alternar janela / tela cheia | F11 ou Alt + Enter | — |
| Monitor de hardware (dev) | F12 (ou parâmetro `--dev`) | — |

Os slots de save state podem ser salvos e carregados diretamente pelo menu de pausa (Esc ou segurando Start).

## Licença

O código do PEnginePs1 está sob a licença [MIT](LICENSE). A licença do código não inclui direitos sobre marcas da Sony, jogos, BIOS, capas ou outros materiais de terceiros.
