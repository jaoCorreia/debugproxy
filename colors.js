const fs = require('fs')
const path = require('path')

const ANSI_CODES = {
  green: '\x1b[32m',
  yellow: '\x1b[33m',
  red: '\x1b[31m',
  cyan: '\x1b[36m',
  magenta: '\x1b[35m',
  blue: '\x1b[34m',
  white: '\x1b[37m',
  dim: '\x1b[2m',
}

const DEFAULT_SERVICE_COLORS = {
  Agriculture: 'green',
  Weather: 'cyan',
  Foreca: 'yellow',
  'Weather.com': 'yellow',
  Keycloak: 'magenta',
  Identity: 'magenta',
  Images: 'dim',
}

const configPath = path.join(__dirname, 'config.json')
let userColors = {}

try {
  if (fs.existsSync(configPath)) {
    const config = JSON.parse(fs.readFileSync(configPath, 'utf-8'))
    userColors = config.colors || {}
  }
} catch {}

const merged = { ...DEFAULT_SERVICE_COLORS, ...userColors }

const SERVICE_COLORS = {}
Object.keys(merged).forEach(label => {
  const code = merged[label]
  SERVICE_COLORS[label] = ANSI_CODES[code] || ANSI_CODES.dim
})

function statusColor(code) {
  if (code >= 500) return '\x1b[31m'
  if (code >= 400) return '\x1b[33m'
  if (code >= 200) return '\x1b[32m'
  return '\x1b[33m'
}

module.exports = {
  ANSI_RESET: '\x1b[0m',
  ANSI_BOLD: '\x1b[1m',
  ANSI_DIM: '\x1b[2m',
  ANSI_GREEN: '\x1b[32m',
  ANSI_YELLOW: '\x1b[33m',
  ANSI_RED: '\x1b[31m',
  ANSI_CYAN: '\x1b[36m',
  ANSI_MAGENTA: '\x1b[35m',
  ANSI_BLUE: '\x1b[34m',
  SERVICE_COLORS,
  statusColor,
  resolveColor(label) {
    return SERVICE_COLORS[label] || null
  },
}
