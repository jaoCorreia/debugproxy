# ◉ ──── ● ──── ◉ Debug Proxy

> HTTP proxy com TUI interativa para debug de aplicações mobile e web.

## Instalação

```bash
git clone <repo>
cd debugproxy
cp routes.example.json routes.json
cp config.example.json config.json
# edite routes.json com suas URLs reais
npm install
```

## Uso

```bash
npm start
```

Inicia na porta `8888` (configurável em `config.json` ou `PORT` env).

## TUI (Terminal UI)

```
+--Sidebar---------------+--Main Area---------------------------+
| YVY Debug Proxy        | [14:06:15] fxbtrz                   |
| Port: 8888             | GET /agr/v1/farms                   |
|                         | -> https://agriculture...           |
| Services                | Response: 200 45ms                 |
| ✓ Logs           (l)   |                                     |
| ✓ Agriculture    (agr) | [14:06:16] LOG {"level":"debug"...}|
| ✓ Weather        (wth) |                                     |
| ...                     |                                     |
| Routes                  |                                     |
| /agr -> Agriculture     |                                     |
| ...                     |                                     |
| File                    |                                     |
| Mode: day               |                                     |
| logs/proxy-2026-07-14..|                                     |
|                         |                                     |
| Keys                    |                                     |
+-------------------------+-------------------------------------+
| > comando                                              (ENTER)|
+--------------------------------------------------------------+
```

### Atalhos

| Tecla | Ação |
|---|---|
| `a` `w` `k` `i` `f` `d` `m` `l` | Toggle serviço (mostrar/esconder) |
| `ENTER` | Abre barra de comando |
| `q` | Sair |
| `jj` | Pular pro fim do log |
| `PgUp`/`PgDn` | Scroll no log |
| `↑` `↓` | Scroll no log |

### Comandos (ENTER)

| Comando | Descrição |
|---|---|
| `all` | Mostrar todos serviços |
| `none` | Esconder todos serviços |
| `status` | Exibir estado dos filtros |
| `add /prefix https://target Label` | Adicionar rota dinâmica (persiste) |
| `rm /prefix` | Remover rota dinâmica |
| `logmode day` | Log por dia (padrão, mesmo arquivo o dia todo) |
| `logmode session` | Log por sessão (arquivo novo a cada start) |

## API REST (para agentes AI)

Base: `http://localhost:8888`

| Método | Rota | Descrição |
|---|---|---|
| `GET` | `/api/status` | Estado atual: filtros, rotas, arquivo de log |
| `POST` | `/api/cmd` | Executa comando (`{"cmd": "fr"}`) |
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

## Integração com OpenCode

```bash
# Monitorar erros no proxy a cada 5 segundos
while true; do
  curl -s http://localhost:8888/api/logs | grep -E "ERROR|5[0-9]{2}" && echo "---"
  sleep 5
done
```

Ou peça ao agente:
> "Deixe um agente monitorando GET http://192.168.13.176:8888/api/logs a cada 5s, me alerte se houver erros"

## Configuração

### `config.json` (gitignored)

```json
{
  "port": 8888,
  "colors": {
    "Agriculture": "green",
    "Weather": "cyan",
    "Foreca": "yellow",
    "Weather.com": "yellow",
    "Keycloak": "magenta",
    "Identity": "magenta",
    "Images": "dim"
  }
}
```

Cores disponíveis: `green`, `yellow`, `red`, `cyan`, `magenta`, `blue`, `white`, `dim`.

### `routes.json` (gitignored)

```json
[
  { "prefix": "/agr", "target": "https://agriculture-service.yvy.ag/api", "label": "Agriculture" },
  { "prefix": "/wth", "target": "https://weather-service-prd.yvy.ag/api", "label": "Weather" }
]
```

Rotas adicionadas via comando `add` são salvas em `routes-dynamic.json` (também gitignored).

### `.env` (opcional)

```
PORT=8888
```

## Logs

- **Por dia** (padrão): `logs/proxy-2026-07-14.txt`
- **Por sessão**: `logs/proxy-2026-07-14_15-30-00.txt`

Modo trocável via comando `logmode day|session`.

## Estrutura

```
debugproxy/
├── app.js                Servidor HTTP + proxy + API
├── tui.js                Interface de terminal (blessed)
├── filters.js            Estado e controle de filtros
├── routes.js             Carregador de rotas (JSON)
├── colors.js             Cores ANSI + config.json
├── fileLogger.js         Escrita em arquivo de log
├── consoleVisualService.js  Banner de startup
├── routes.example.json   Template de rotas (comitado)
├── config.example.json   Template de config (comitado)
├── routes.json           Suas rotas (gitignored)
├── config.json           Suas cores (gitignored)
├── routes-dynamic.json   Rotas adicionadas em runtime (gitignored)
├── logs/                 Arquivos de sessão (gitignored)
├── .gitignore
├── package.json
└── README.md
```
