const { getRoutes } = require('./routes')
const { ANSI_RESET, ANSI_BOLD, ANSI_DIM, ANSI_GREEN, ANSI_RED, ANSI_YELLOW } = require('./colors')

const LOGS_KEY = 'Logs'

const filters = {}
const FILTER_ALIASES = {}

function buildFirstLetters(labels) {
  const counts = {}
  labels.forEach(l => {
    const ch = l[0].toLowerCase()
    counts[ch] = (counts[ch] || 0) + 1
  })
  return Object.keys(counts).filter(ch => counts[ch] === 1)
}

function rebuild() {
  const labels = getRoutes().map(r => r.label)
  const state = {}
  labels.forEach(l => {
    state[l] = filters[l] !== undefined ? filters[l] : true
  })
  state[LOGS_KEY] = filters[LOGS_KEY] !== undefined ? filters[LOGS_KEY] : true

  const aliases = {}
  getRoutes().forEach(r => {
    const short = r.prefix.replace('/', '')
    aliases[short] = r.label
  })
  aliases['l'] = LOGS_KEY

  const allLabels = [LOGS_KEY, ...labels]
  const unique = buildFirstLetters(allLabels)
  unique.forEach(ch => {
    const match = allLabels.find(l => l[0].toLowerCase() === ch)
    if (match) aliases[ch] = match
  })

  // Mutação in-place: tui.js/app.js importam estas referências no require.
  // Reatribuir (filters = state) criaria objetos novos e deixaria as
  // referências importadas obsoletas — toggles paravam de funcionar após
  // adicionar/remover rota.
  Object.keys(filters).forEach(k => delete filters[k])
  Object.assign(filters, state)
  Object.keys(FILTER_ALIASES).forEach(k => delete FILTER_ALIASES[k])
  Object.assign(FILTER_ALIASES, aliases)
}

rebuild()

function shouldShow(routeLabel) {
  if (!routeLabel) return true
  const key = routeLabel === 'LOG' ? LOGS_KEY : routeLabel
  return filters[key] !== false
}

function handleCommand(cmd) {
  if (cmd === 'all') {
    Object.keys(filters).forEach(k => (filters[k] = true))
    return
  }
  if (cmd === 'none') {
    Object.keys(filters).forEach(k => (filters[k] = false))
    return
  }
  const key = FILTER_ALIASES[cmd]
  if (key && key in filters) {
    filters[key] = !filters[key]
  }
}

module.exports = { filters, FILTER_ALIASES, shouldShow, handleCommand, rebuild }
