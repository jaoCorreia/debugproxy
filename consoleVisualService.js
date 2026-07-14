const ascii = require('./asciiArt')
const { SERVICE_COLORS } = require('./colors')

const ANSI_TO_ASCII_COLOR = {
  '\x1b[32m': 'green',
  '\x1b[33m': 'yellow',
  '\x1b[34m': 'blue',
  '\x1b[35m': 'magenta',
  '\x1b[36m': 'cyan',
  '\x1b[2m': 'dim',
}

function printBanner(proxyPort, routes) {
  process.stdout.write('\n')
  process.stdout.write(`  ${ascii.paint('\u25C9 \u2500\u2500\u2500\u2500 \u25CF \u2500\u2500\u2500\u2500 \u25C9', 'cyan', 'bold')}\n`)
  process.stdout.write(`    ${ascii.paint('DEBUG PROXY', 'yellow', 'bold')}\n\n`)
  process.stdout.write(`  ${ascii.paint('Port:', 'dim')} ${ascii.paint(String(proxyPort), 'cyan', 'bold')}  ${ascii.paint('Routes:', 'dim')} ${ascii.paint(String(routes.length), 'green', 'bold')}\n\n`)

  for (const r of routes) {
    const ansi = SERVICE_COLORS[r.label]
    const ac = ANSI_TO_ASCII_COLOR[ansi] || 'white'
    process.stdout.write(`    ${ascii.paint(r.prefix, 'white', 'bold')} ${ascii.paint('\u2192', 'dim')} ${ascii.paint(r.label, ac)}\n`)
    process.stdout.write(`    ${ascii.paint(r.target, 'dim')}\n`)
  }
  process.stdout.write('\n')
}

module.exports = printBanner
