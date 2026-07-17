# o ---- * ---- o Debug Proxy (Rust)

> HTTP proxy com TUI interativa (ratatui) para debug de aplicações mobile e web.
> Reescrita em Rust da versão Node.js.

## Download

Baixe o binário pronto para seu sistema na [página de releases](https://github.com/jaoCorreia/debugproxy/releases/latest):

| Plataforma | Download |
|---|---|
| 🐧 Linux (x86_64) | [debugproxy-linux-x86_64.tar.gz](https://github.com/jaoCorreia/debugproxy/releases/latest/download/debugproxy-linux-x86_64.tar.gz) |
| 🪟 Windows (x86_64) | [debugproxy-windows-x86_64.zip](https://github.com/jaoCorreia/debugproxy/releases/latest/download/debugproxy-windows-x86_64.zip) |
| 🍎 macOS (Apple Silicon) | [debugproxy-macos-aarch64.tar.gz](https://github.com/jaoCorreia/debugproxy/releases/latest/download/debugproxy-macos-aarch64.tar.gz) |

### Windows

> No Windows, **não dê duplo clique no `.exe`**. Use o `debugproxy.bat` incluído no `.zip`. Ele evita o bloqueio do SmartScreen e mantém a janela aberta se der erro.

1. Extraia o `.zip` para uma pasta
2. Copie `config.example.json` → `config.json` (edite se quiser mudar a porta)
3. Copie `routes.example.json` → `routes.json` (adicione suas rotas)
4. Execute `debugproxy.bat` (duplo clique ou pelo terminal)
5. Se o Windows SmartScreen bloquear, clique em **"Mais informações" → "Executar mesmo assim"**, ou use o `.bat`

Requer **Windows 10 build 1909+** e **Windows Terminal** (recomendado para cores e Unicode).

### Linux / macOS

```bash
tar xzf debugproxy-*.tar.gz
cp config.example.json config.json
cp routes.example.json routes.json
./debugproxy
```

No macOS, se o Gatekeeper bloquear, execute `./remove-quarantine.sh` primeiro.

### Build from source

```bash
git clone https://github.com/jaoCorreia/debugproxy.git
cd debugproxy
cp routes.example.json routes.json
cp config.example.json config.json
cargo build --release
```

## Uso

```bash
cargo run --release
# ou
./target/release/debugproxy
```

Inicia na porta `8888` (configurável em `config.json` ou `PORT` env).
Os arquivos `routes.json`/`config.json` são lidos do diretório de trabalho atual.

## TUI (Terminal UI)

```
+--Sidebar---------------+--Main Area---------------------------+
|   o ---- * ---- o      | [14:06:15] fxbtrz                   |
|     DEBUG PROXY        | GET /agr/v1/farms                   |
| Port: 8888             | -> https://agriculture...           |
|                        | Response: 200 45ms                  |
| Services               |                                     |
| ✓ Logs          (l)    | [14:06:16] LOG {"level":"debug"...} |
| ✓ Agriculture   (agr)  |                                     |
| ✓ Weather       (wth)  |                                     |
| ...                    |                                     |
| Routes                 |                                     |
| /agr -> Agriculture    |                                     |
| ...                    |                                     |
| File                   |                                     |
| Mode: day              |                                     |
| logs/proxy-2026-07-15..|                                     |
|                        |                                     |
| Keys                   |                                     |
+------------------------+-------------------------------------+
| > comando                                             (ENTER) |
+----------------------------------------------------------------+
```

### Atalhos

| Tecla | Ação |
|---|---|
| `a` `w` `k` `i` `f` `d` `m` `l` | Toggle serviço (mostrar/esconder) |
| `ENTER` | Abre barra de comando |
| `q` | Sair |
| `j` | Pular pro fim do log |
| `PgUp`/`PgDn` | Scroll no log (10 linhas) |
| `↑` `↓` | Scroll no log |
| `ESC` | Fecha barra de comando |

### Comandos (ENTER)

| Comando | Descrição |
|---|---|
| `all` | Mostrar todos serviços |
| `none` | Esconder todos serviços |
| `add /prefix https://target Label` | Adicionar rota dinâmica (persiste) |
| `rm /prefix` | Remover rota dinâmica |
| `logmode day` | Log por dia (padrão, mesmo arquivo o dia todo) |
| `logmode session` | Log por sessão (arquivo novo a cada start) |
| `saver [cena]` | Liga o screensaver (`starfield`, `rain`, `particles`) |

## API REST (para agentes AI)

Base: `http://localhost:8888`

| Método | Rota | Descrição |
|---|---|---|
| `GET` | `/api/status` | Estado atual: filtros, rotas, arquivo de log |
| `POST` | `/api/cmd` | Executa comando (`{"cmd": "agr"}`) |
| `GET` | `/api/logs` | Últimas 50 linhas do arquivo de log |
| `GET` | `/health` | Health check |
| `POST` | `/log` | Recebe logs do app mobile |
| `*` | `/agr/*`, `/wth/*`, etc | Proxy das APIs configuradas |

### Exemplos API

```bash
# Ver estado
curl http://localhost:8888/api/status

# Esconder tudo e mostrar só Agriculture
curl -X POST http://localhost:8888/api/cmd -H "Content-Type: application/json" -d '{"cmd":"none"}'
curl -X POST http://localhost:8888/api/cmd -H "Content-Type: application/json" -d '{"cmd":"agr"}'

# Adicionar rota
curl -X POST http://localhost:8888/api/cmd -H "Content-Type: application/json" -d '{"cmd":"add /sub https://subscription.yvy.ag Subscription"}'

# Tail dos logs
curl http://localhost:8888/api/logs
```

## Configuração

### `config.json` (gitignored)

```json
{
  "port": 8888,
  "colors": {
    "Agriculture": "green",
    "Weather": "cyan"
  },
  "screensaver": {
    "enabled": true,
    "idleSeconds": 90,
    "fps": 20,
    "cycleSeconds": 60,
    "fadeSeconds": 0.6,
    "theme": "cosmic",
    "scenes": "all",
    "wakeOnLog": true
  }
}
```

Cores disponíveis: `green`, `yellow`, `red`, `cyan`, `magenta`, `blue`, `white`, `dim`.
Temas do screensaver: `cosmic`, `nord`, `dracula`, `gruvbox`, `forest`, `mono`.

### `routes.json` (gitignored)

```json
[
  { "prefix": "/agr", "target": "https://agriculture-service.yvy.ag/api", "label": "Agriculture" },
  { "prefix": "/wth", "target": "https://weather-service-prd.yvy.ag/api", "label": "Weather" }
]
```

Rotas adicionadas via comando `add` são salvas em `routes-dynamic.json` (também gitignored).

## Logs

- **Por dia** (padrão): `logs/proxy-2026-07-15.txt`
- **Por sessão**: `logs/proxy-2026-07-15_15-30-00.txt`

Modo trocável via comando `logmode day|session`.

## Estrutura

```
debugproxy-rs/
├── src/
│   ├── main.rs           Entry point: estado + runtime tokio + TUI
│   ├── proxy.rs          Servidor HTTP (axum) + proxy (reqwest) + API
│   ├── tui.rs            Interface de terminal (ratatui)
│   ├── screensaver.rs    Cenas starfield/rain/particles + temas
│   ├── filters.rs        Estado e controle de filtros
│   ├── routes.rs         Carregador de rotas (JSON)
│   ├── colors.rs         Cores ANSI + config.json
│   ├── logger.rs         Escrita em arquivo de log
│   ├── config.rs         Carregador de config.json
│   └── state.rs          Estado compartilhado (AppState)
├── routes.example.json   Template de rotas (comitado)
├── config.example.json   Template de config (comitado)
├── routes.json           Suas rotas (gitignored)
├── config.json           Suas cores (gitignored)
├── routes-dynamic.json   Rotas adicionadas em runtime (gitignored)
├── logs/                 Arquivos de log (gitignored)
└── Cargo.toml
```

## Diferenças vs versão Node.js

- O path do `target` é respeitado integralmente ao encaminhar (a versão JS
  descartava o path do target — ex. `/api` — e usava só o host).
- Mensagens de comando da TUI (`+ Route`, `Log mode`) também vão pro arquivo de log.
- `j` (única tecla) pula pro fim do log.

## Limitações conhecidas

- **Compressão**: o proxy negocia a compressão com o upstream por conta própria
  (gzip/brotli/deflate) e entrega o corpo **descomprimido** ao cliente — o
  `accept-encoding` do cliente não é repassado. Isso mantém os logs legíveis e
  evita bytes binários corrompendo a TUI, mas fluxos que dependem de testar a
  negociação de compressão fim-a-fim não são observáveis através do proxy.
- **Respostas grandes**: corpos de resposta (pós-descompressão) são limitados a
  64MB; acima disso o proxy responde 502. Downloads muito grandes devem ser
  feitos fora do proxy.
- **Streaming (SSE/long-polling)**: respostas são bufferizadas por inteiro antes
  do repasse, então Server-Sent Events não funcionam através do proxy.
