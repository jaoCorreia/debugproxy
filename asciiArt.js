const readline = require('readline')

const BLACK = '\x1b[30m'
const RED = '\x1b[31m'
const GREEN = '\x1b[32m'
const YELLOW = '\x1b[33m'
const BLUE = '\x1b[34m'
const MAGENTA = '\x1b[35m'
const CYAN = '\x1b[36m'
const WHITE = '\x1b[37m'
const RESET = '\x1b[0m'
const BOLD = '\x1b[1m'
const DIM = '\x1b[2m'
const HIDE_CURSOR = '\x1b[?25l'
const SHOW_CURSOR = '\x1b[?25h'

const COLORS = { black: BLACK, red: RED, green: GREEN, yellow: YELLOW, blue: BLUE, magenta: MAGENTA, cyan: CYAN, white: WHITE }

function paint(text, color, style) {
  let out = ''
  if (style === 'bold') out += BOLD
  if (style === 'dim') out += DIM
  if (COLORS[color]) out += COLORS[color]
  out += text + RESET
  return out
}

function typewrite(text, { speed = 30, color = 'white', style } = {}, onDone) {
  process.stdout.write(HIDE_CURSOR)
  let i = 0
  const interval = setInterval(() => {
    process.stdout.write(paint(text[i], color, style))
    i++
    if (i >= text.length) {
      clearInterval(interval)
      process.stdout.write(SHOW_CURSOR)
      if (onDone) onDone()
    }
  }, speed)
}

function render(frame, { x = 0, color = 'cyan', style } = {}) {
  const c = COLORS[color] || ''
  const s = style === 'bold' ? BOLD : style === 'dim' ? DIM : ''
  const prefix = s + c
  const lines = frame.split('\n')
  lines.forEach((line, i) => {
    readline.cursorTo(process.stdout, x)
    if (i > 0) readline.clearLine(process.stdout, 0)
    process.stdout.write(`${prefix}${line}${RESET}\n`)
  })
}

function play({ frames, fps = 10, x = 0, loop = false }) {
  process.stdout.write(HIDE_CURSOR)
  let idx = 0
  const interval = setInterval(() => {
    readline.cursorTo(process.stdout, x)

    const targetLines = frames[idx].split('\n').length
    for (let i = 0; i < targetLines; i++) {
      readline.cursorTo(process.stdout, x)
      readline.clearLine(process.stdout, 0)
      if (i < targetLines - 1) readline.moveCursor(process.stdout, 0, 1)
    }
    readline.moveCursor(process.stdout, 0, -targetLines + 1)

    render(frames[idx], { x })
    idx++
    if (idx >= frames.length) {
      if (loop) {
        idx = 0
      } else {
        clearInterval(interval)
        process.stdout.write(SHOW_CURSOR)
      }
    }
  }, 1000 / fps)
}

module.exports = { paint, typewrite, render, play, COLORS }
