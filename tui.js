const blessed = require('blessed')
const { filters, FILTER_ALIASES, handleCommand, rebuild } = require('./filters')
const { getRoutes, addRoute, removeRoute } = require('./routes')
const fileLogger = require('./fileLogger')

let screen
let sidebar
let logBox
let commandBar
let mode = 'view'
let PROXY_PORT = 8888

function renderSidebar() {
  const routes = getRoutes()
  let content = '{bold}{cyan-fg}YVY Debug Proxy{/cyan-fg}{/bold}\n'
  content += `Port: ${PROXY_PORT}\n\n`

  content += '{bold}Services{/bold}\n'
  const labels = Object.keys(filters)
  labels.forEach(label => {
    const route = routes.find(r => r.label === label)
    const alias = label === 'Logs' ? 'l' : route?.prefix?.replace('/', '') || label[0].toLowerCase()
    const marker = filters[label] ? '{green-fg}\u2713{/green-fg}' : '{red-fg}\u2717{/red-fg}'
    content += ` ${marker} ${label.padEnd(13)}(${alias})\n`
  })

  content += '\n{bold}Routes{/bold}\n'
  routes.forEach(r => {
    content += `${r.prefix} \u2192 ${r.label}\n`
  })

  const sessionFile = fileLogger.getSessionFile() || '(aguardando...)'
  content += '\n{bold}File{/bold}\n'
  content += `Mode: ${fileLogger.getMode()}\n`
  content += `${sessionFile}\n`

  content += '\n{bold}Keys{/bold}\n'
  content += 'keys: toggle service\n'
  content += 'ENTER: command mode\n'
  content += '  all, none, status\n'
  content += '  add /pref URL Label\n'
  content += '  rm /pref\n'
  content += '  logmode day|session\n'
  content += 'q: quit  jj: jump to bottom\n'

  sidebar.setContent(content)
  screen.render()
}

function setupKeys() {
  screen.key(['q', 'C-c'], () => {
    if (mode !== 'view') return
    screen.destroy()
    process.exit(0)
  })

  screen.key(['j', 'j'], () => {
    if (mode !== 'view') return
    logBox.setScrollPerc(100)
    screen.render()
  })

  // Atalhos de uma tecla resolvidos dinamicamente contra FILTER_ALIASES:
  // rota adicionada em runtime (add /pref ...) ganha alias na hora, sem
  // reiniciar. Só valem fora do modo comando — senão digitar na barra
  // também dispararia os toggles.
  screen.on('keypress', ch => {
    if (mode !== 'view' || !ch || ch.length !== 1) return
    const label = FILTER_ALIASES[ch]
    if (label && label in filters) {
      filters[label] = !filters[label]
      renderSidebar()
    }
  })

  screen.key(['enter'], () => {
    if (mode !== 'view') return
    mode = 'cmd'
    commandBar.setValue('')
    commandBar.readInput((err, value) => {
      mode = 'view'
      // Sempre limpa a barra ao sair do modo comando — o valor digitado
      // ficava exibido depois do ENTER.
      commandBar.setValue('')
      if (err || !value || !value.trim()) {
        logBox.focus()
        screen.render()
        return
      }
      const cmd = value.trim()
      const parts = cmd.split(/\s+/)
      const action = parts[0].toLowerCase()

      if (action === 'add' && parts.length >= 3) {
        const prefix = parts[1]
        const target = parts[2]
        const label = parts.slice(3).join(' ') || prefix.replace(/^\//, '').toUpperCase()
        const result = addRoute(prefix, target, label)
        if (result.ok) {
          rebuild()
          logBox.add(`{green-fg}+ Route: ${prefix} \u2192 ${target}{/green-fg}`)
        } else {
          logBox.add(`{red-fg}${result.error}{/red-fg}`)
        }
      } else if (action === 'rm' && parts.length >= 2) {
        const result = removeRoute(parts[1])
        if (result.ok) {
          rebuild()
          logBox.add(`{yellow-fg}- Route: ${parts[1]}{/yellow-fg}`)
        } else {
          logBox.add(`{red-fg}${result.error}{/red-fg}`)
        }
      } else if (action === 'logmode' && parts.length >= 2) {
        if (parts[1] === 'day' || parts[1] === 'session') {
          fileLogger.setMode(parts[1])
          logBox.add(`{green-fg}Log mode: ${parts[1]}{/green-fg}`)
        } else {
          logBox.add(`{yellow-fg}Use: logmode day|session{/yellow-fg}`)
        }
      } else {
        handleCommand(cmd)
      }
      renderSidebar()
      logBox.focus()
      screen.render()
    })
  })

  screen.key(['escape'], () => {
    mode = 'view'
    commandBar.setValue('')
    screen.render()
  })
}

function init(port, routes) {
  PROXY_PORT = port
  fileLogger.initSession()

  screen = blessed.screen({ smartCSR: true, title: 'YVY Debug Proxy', dockBorders: false, fullUnicode: true })

  const SIDEBAR_WIDTH = 30

  sidebar = blessed.box({
    parent: screen, left: 0, top: 0, width: SIDEBAR_WIDTH, height: '100%-3',
    border: { type: 'line', fg: 'cyan' },
    style: { fg: 'white', bg: 'black' },
    padding: { left: 1, right: 1 },
    scrollable: true,
    keys: true,
    vi: true,
    // Sem tags:true o blessed imprime {bold}/{cyan-fg} como texto literal.
    tags: true,
  })

  logBox = blessed.log({
    parent: screen, left: SIDEBAR_WIDTH, top: 0, width: `100%-${SIDEBAR_WIDTH}`, height: '100%-3',
    border: { type: 'line', fg: 'cyan' },
    style: { fg: 'white', bg: 'black' },
    scrollable: true, scrollback: 10000,
    mouse: true, keys: true, vi: true,
    tags: true,
  })

  // height 3: a borda ocupa 2 linhas — com height 1 o campo de digitação
  // ficava invisível (só a borda aparecia).
  // Sem inputOnFocus: o readInput() explícito no ENTER já registra o leitor;
  // com os dois ativos cada tecla era capturada duas vezes (ww tt hh).
  commandBar = blessed.textbox({
    parent: screen, left: 0, bottom: 0, width: '100%', height: 3,
    border: { type: 'line', fg: 'cyan' },
    style: { fg: 'yellow', bg: 'black' },
    keys: true,
  })

  renderSidebar()
  setupKeys()
  logBox.focus()
  screen.render()
}

function log(...args) {
  const text = args.map(a => (typeof a === 'string' ? a : JSON.stringify(a))).join(' ')
  // Arquivo recebe a versão sem ANSI; o painel recebe o original — o blessed
  // renderiza códigos ANSI nativamente, então as cores do colors.js aparecem.
  const stripped = text.replace(/\x1b\[[0-9;]*m/g, '')
  fileLogger.append(stripped)

  if (logBox) {
    logBox.add(text)
    screen.render()
  }
}

function updateSidebar() {
  renderSidebar()
}

module.exports = { init, log, updateSidebar }
