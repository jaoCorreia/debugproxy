const ascii = require('./asciiArt')
const { SERVICE_COLORS } = require('./colors')

const LOGO = [
  '  \u250C\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2510       \u250C\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2510',
  '  \u2502  \u25C9 \u2502\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2502  \u25C9 \u2502',
  '  \u2514\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2518       \u2514\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2518',
  '',
  '  D E B U G   P R O X Y',
]

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

  LOGO.forEach((line, i) => {
    if (i === 3) return process.stdout.write('\n')
    const color = i === 4 ? 'yellow' : i <= 2 ? 'cyan' : 'white'
    const style = i === 4 ? 'bold' : 'dim'
    process.stdout.write(`  ${ascii.paint(line, color, style)}\n`)
  })

  process.stdout.write('\n')
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
