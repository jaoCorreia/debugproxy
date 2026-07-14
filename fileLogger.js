const fs = require('fs')
const path = require('path')

const LOG_DIR = path.join(__dirname, 'logs')

let mode = 'day'
let currentFile = null
let currentDay = null

function stripAnsi(str) {
  return str.replace(/\x1b\[[0-9;]*m/g, '')
}

function ensureDir() {
  if (!fs.existsSync(LOG_DIR)) {
    fs.mkdirSync(LOG_DIR, { recursive: true })
  }
}

function dayKey(d) {
  const pad = n => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

function sessionKey(d) {
  const pad = n => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}_${pad(d.getHours())}-${pad(d.getMinutes())}-${pad(d.getSeconds())}`
}

function resolveFile() {
  const now = new Date()
  if (mode === 'day') {
    const today = dayKey(now)
    if (currentDay !== today) {
      currentDay = today
      currentFile = path.join(LOG_DIR, `proxy-${today}.txt`)
    }
  } else if (!currentFile) {
    currentFile = path.join(LOG_DIR, `proxy-${sessionKey(now)}.txt`)
  }
  return currentFile
}

function initSession() {
  ensureDir()
  resolveFile()
  const header = `\u2500\u2500\u2500 ${mode === 'day' ? 'Day' : 'Session'} started at ${new Date().toISOString()} \u2500\u2500\u2500`
  fs.appendFileSync(currentFile, stripAnsi(header) + '\n', 'utf-8')
}

function append(text) {
  ensureDir()
  resolveFile()
  fs.appendFileSync(currentFile, stripAnsi(text) + '\n', 'utf-8')
}

function writeLine(text) {
  append(text)
}

function setMode(newMode) {
  mode = newMode
  currentFile = null
  currentDay = null
  resolveFile()
}

function getMode() {
  return mode
}

module.exports = {
  append,
  writeLine,
  initSession,
  getSessionFile: () => currentFile,
  setMode,
  getMode,
}
