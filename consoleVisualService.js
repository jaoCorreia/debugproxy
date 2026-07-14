const { ANSI_BOLD, ANSI_RESET, ANSI_DIM, ANSI_CYAN, ANSI_MAGENTA, ANSI_GREEN, SERVICE_COLORS } = require('./colors')

function printBanner(proxyPort, routes) {
  console.log('')
  console.log(`  ${ANSI_CYAN}  \u256D\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u256E${ANSI_RESET}`)
  console.log(`  ${ANSI_CYAN}  \u2502${ANSI_RESET}   ${ANSI_BOLD}${ANSI_MAGENTA}\u25C9${ANSI_RESET}  ${ANSI_BOLD}D E B U G   P R O X Y${ANSI_RESET}   ${ANSI_CYAN}  \u2502${ANSI_RESET}`)
  console.log(`  ${ANSI_CYAN}  \u2570\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u256F${ANSI_RESET}`)
  console.log('')
  console.log(`  ${ANSI_DIM}Port:${ANSI_RESET} ${ANSI_BOLD}${proxyPort}${ANSI_RESET}  ${ANSI_DIM}Routes:${ANSI_RESET} ${ANSI_BOLD}${routes.length}${ANSI_RESET}`)
  console.log('')
  for (const r of routes) {
    const color = SERVICE_COLORS[r.label] || ''
    console.log(`    ${ANSI_BOLD}${r.prefix}${ANSI_RESET} ${ANSI_DIM}\u2192${ANSI_RESET} ${color}${r.label}${ANSI_RESET}`)
    console.log(`    ${ANSI_DIM}${r.target}${ANSI_RESET}`)
  }
  console.log('')
}

module.exports = printBanner
