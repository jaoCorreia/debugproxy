// Screensaver de idle para a TUI — técnicas portadas do drift
// (github.com/phlx0/drift): loop update/draw com dt, temas RGB com
// paleta/dim/bright, lerp de cor, transições fade e cenas starfield,
// rain e particles.
const blessed = require('blessed')

// ---------------------------------------------------------------------------
// Temas (paletas copiadas do drift internal/scene/scene.go)

const THEMES = {
  cosmic: {
    palette: [[100, 140, 230], [160, 100, 220], [80, 200, 220], [180, 140, 255]],
    dim: [[25, 35, 70], [40, 22, 60], [18, 50, 60], [45, 30, 70]],
    bright: [230, 235, 255],
  },
  nord: {
    palette: [[136, 192, 208], [129, 161, 193], [143, 188, 187], [163, 190, 140]],
    dim: [[46, 52, 64], [59, 66, 82], [67, 76, 94], [76, 86, 106]],
    bright: [236, 239, 244],
  },
  dracula: {
    palette: [[189, 147, 249], [255, 121, 198], [139, 233, 253], [80, 250, 123]],
    dim: [[48, 34, 78], [68, 28, 52], [32, 58, 68], [18, 62, 32]],
    bright: [248, 248, 242],
  },
  gruvbox: {
    palette: [[251, 189, 35], [184, 187, 38], [214, 93, 14], [104, 157, 106]],
    dim: [[58, 44, 8], [44, 46, 10], [50, 22, 4], [26, 38, 24]],
    bright: [235, 219, 178],
  },
  forest: {
    palette: [[80, 200, 90], [60, 160, 100], [160, 220, 80], [40, 180, 140]],
    dim: [[14, 38, 16], [12, 30, 20], [33, 48, 14], [10, 38, 28]],
    bright: [200, 240, 180],
  },
  mono: {
    palette: [[0, 200, 80], [0, 170, 65], [0, 230, 95], [0, 150, 55]],
    dim: [[0, 38, 14], [0, 28, 11], [0, 46, 16], [0, 24, 9]],
    bright: [180, 255, 200],
  },
}

function lerp(a, b, t) {
  if (t < 0) t = 0
  if (t > 1) t = 1
  return [
    Math.round(a[0] * (1 - t) + b[0] * t),
    Math.round(a[1] * (1 - t) + b[1] * t),
    Math.round(a[2] * (1 - t) + b[2] * t),
  ]
}

// ---------------------------------------------------------------------------
// Framebuffer de células — cada frame vira uma string ANSI truecolor que o
// blessed converte para a cor 256 mais próxima (screen.attrCode).

class Buffer {
  constructor(w, h) {
    this.w = w
    this.h = h
    this.ch = new Array(w * h)
    this.color = new Array(w * h)
    this.clear()
  }

  clear() {
    this.ch.fill(null)
    this.color.fill(null)
  }

  set(x, y, ch, color) {
    if (x < 0 || x >= this.w || y < 0 || y >= this.h) return
    const i = y * this.w + x
    this.ch[i] = ch
    this.color[i] = color
  }

  // fade: 0 = visível, 1 = preto total (técnica do fadeScreen do drift)
  toContent(fade) {
    const t = 1 - Math.max(0, Math.min(1, fade))
    const rows = new Array(this.h)
    for (let y = 0; y < this.h; y++) {
      let row = ''
      let last = null
      for (let x = 0; x < this.w; x++) {
        const i = y * this.w + x
        if (!this.ch[i]) {
          row += ' '
          continue
        }
        const c = this.color[i]
        const code = `${Math.round(c[0] * t)};${Math.round(c[1] * t)};${Math.round(c[2] * t)}`
        if (code !== last) {
          row += `\x1b[38;2;${code}m`
          last = code
        }
        row += this.ch[i]
      }
      rows[y] = row
    }
    return rows.join('\n') + '\x1b[m'
  }
}

const rand = Math.random
const randInt = n => Math.floor(Math.random() * n)

// ---------------------------------------------------------------------------
// Cena: starfield (port de internal/scene/starfield/starfield.go)

const SF_GLYPHS = ['\u00b7', '\u2218', '*', '\u2726']

class Starfield {
  constructor() { this.name = 'starfield' }

  init(w, h, theme) {
    this.w = w
    this.h = h
    this.theme = theme
    this.stars = []
    for (let i = 0; i < 160; i++) this.stars.push(this.spawn(true))
  }

  spawn(scattered) {
    return {
      x: rand() * 2 - 1,
      y: rand() * 2 - 1,
      z: scattered ? 0.02 + rand() * 0.98 : 0.75 + rand() * 0.25,
      prevPX: 0, prevPY: 0, hasPrev: false,
      palIdx: randInt(this.theme.palette.length),
    }
  }

  project(st) {
    const px = Math.round(this.w * 0.5 + (st.x / st.z) * this.w * 0.5)
    const py = Math.round(this.h * 0.5 + (st.y / st.z) * this.h * 0.5)
    return [px, py, px >= 0 && px < this.w && py >= 0 && py < this.h]
  }

  update(dt) {
    const speed = 0.3
    for (let i = 0; i < this.stars.length; i++) {
      const st = this.stars[i]
      const [px, py, ok] = this.project(st)
      if (ok) { st.prevPX = px; st.prevPY = py; st.hasPrev = true }
      else st.hasPrev = false

      st.z -= speed * dt
      if (st.z <= 0.01 || !this.project(st)[2]) this.stars[i] = this.spawn(false)
    }
  }

  draw(buf) {
    const { palette, dim, bright } = this.theme
    for (const st of this.stars) {
      const [px, py, ok] = this.project(st)
      if (!ok) continue

      const b = Math.pow(1 - st.z, 1.5)
      const pal = palette[st.palIdx % palette.length]
      const dm = dim[st.palIdx % dim.length]

      if (st.hasPrev && (st.prevPX !== px || st.prevPY !== py)) {
        buf.set(st.prevPX, st.prevPY, '\u00b7', lerp(dm, pal, b * 0.35))
      }

      const glyph = SF_GLYPHS[Math.min(Math.floor(b * SF_GLYPHS.length), SF_GLYPHS.length - 1)]
      const color = b > 0.85 ? lerp(pal, bright, (b - 0.85) / 0.15) : lerp(dm, pal, b / 0.85)
      buf.set(px, py, glyph, color)
    }
  }
}

// ---------------------------------------------------------------------------
// Cena: rain / matrix (port de internal/scene/rain/rain.go)

const RAIN_CHARSET = [...'\uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79\uff7a\uff7b\uff7c\uff7d\uff7e\uff7f\uff80\uff81\uff82\uff83\uff84\uff85\uff86\uff87\uff88\uff890123456789']

class Rain {
  constructor() { this.name = 'rain' }

  init(w, h, theme) {
    this.w = w
    this.h = h
    this.theme = theme
    this.grid = new Float64Array(w * h)
    this.drops = []
    const count = Math.floor((w * 0.4) / 1.2) + 5
    for (let i = 0; i < count; i++) this.drops.push(this.newDrop(true))
  }

  newDrop(scattered) {
    return {
      col: randInt(this.w),
      y: scattered ? rand() * this.h : 0,
      speed: 5 + rand() * 12,
      headChar: RAIN_CHARSET[randInt(RAIN_CHARSET.length)],
      frameAge: 0,
    }
  }

  update(dt) {
    const decay = dt * 3.5
    for (let i = 0; i < this.grid.length; i++) {
      const v = this.grid[i] - decay
      this.grid[i] = v > 0 ? v : 0
    }

    const trailLen = 14
    for (let i = 0; i < this.drops.length; i++) {
      const d = this.drops[i]
      d.y += d.speed * dt
      const headY = Math.floor(d.y)
      for (let t = 0; t < trailLen; t++) {
        const cy = headY - t
        if (cy >= 0 && cy < this.h) {
          const b = 1 - t / trailLen
          const idx = cy * this.w + d.col
          if (b > this.grid[idx]) this.grid[idx] = b
        }
      }
      d.frameAge++
      if (d.frameAge >= 3 + randInt(4)) {
        d.headChar = RAIN_CHARSET[randInt(RAIN_CHARSET.length)]
        d.frameAge = 0
      }
      if (d.y > this.h + trailLen) this.drops[i] = this.newDrop(false)
    }
  }

  draw(buf) {
    const { palette, dim, bright } = this.theme
    for (let y = 0; y < this.h; y++) {
      for (let x = 0; x < this.w; x++) {
        const b = this.grid[y * this.w + x]
        if (b < 0.04) continue
        const pIdx = x % palette.length
        let ch, color
        if (b > 0.85) { ch = '\u2502'; color = lerp(palette[pIdx], bright, (b - 0.85) / 0.15) }
        else if (b > 0.55) { ch = '\u254e'; color = palette[pIdx] }
        else if (b > 0.3) { ch = '\u254c'; color = lerp(dim[pIdx], palette[pIdx], (b - 0.3) / 0.25) }
        else { ch = '\u00b7'; color = lerp(dim[pIdx], palette[pIdx], b / 0.3) }
        buf.set(x, y, ch, color)
      }
    }
    for (const d of this.drops) {
      const hy = Math.floor(d.y)
      if (hy >= 0 && hy < this.h) buf.set(d.col, hy, d.headChar, bright)
    }
  }
}

// ---------------------------------------------------------------------------
// Cena: particles com flow field (port de internal/scene/particles/particles.go)

const P_GLYPHS = ['\u25e6', '\u00b7', '\u25cb', '\u2022', '.', '\u00b0', '\u2218']

function flowField(x, y, t) {
  return [
    Math.sin(x * 0.04 + t * 0.25) * Math.cos(y * 0.06 + t * 0.18) * 0.6,
    Math.cos(x * 0.06 + t * 0.2) * Math.sin(y * 0.04 + t * 0.22) * 0.6,
  ]
}

class Particles {
  constructor() { this.name = 'particles' }

  init(w, h, theme) {
    this.w = w
    this.h = h
    this.theme = theme
    this.time = 0
    this.trail = new Float64Array(w * h)
    this.particles = []
    for (let i = 0; i < 120; i++) this.particles.push(this.newParticle(true))
  }

  newParticle(scattered) {
    const speed = 0.4 + rand() * 2.2
    const angle = rand() * 2 * Math.PI
    let x = 0, y = 0
    if (scattered) {
      x = rand() * this.w
      y = rand() * this.h
    } else {
      switch (randInt(4)) {
        case 0: x = rand() * this.w; y = 0; break
        case 1: x = rand() * this.w; y = this.h - 1; break
        case 2: x = 0; y = rand() * this.h; break
        default: x = this.w - 1; y = rand() * this.h
      }
    }
    return {
      x, y,
      vx: Math.cos(angle) * speed,
      vy: Math.sin(angle) * speed,
      glyph: P_GLYPHS[randInt(P_GLYPHS.length)],
      palIdx: randInt(this.theme.palette.length),
      phase: rand() * 2 * Math.PI,
    }
  }

  update(dt) {
    this.time += dt
    const decay = dt * 2.8
    for (let i = 0; i < this.trail.length; i++) {
      const v = this.trail[i] - decay
      this.trail[i] = v > 0 ? v : 0
    }

    const friction = Math.pow(0.98, dt * 60)
    for (let i = 0; i < this.particles.length; i++) {
      const p = this.particles[i]
      const [fx, fy] = flowField(p.x, p.y, this.time)
      p.vx = (p.vx + fx * dt) * friction
      p.vy = (p.vy + fy * dt) * friction

      const speed = Math.sqrt(p.vx * p.vx + p.vy * p.vy)
      if (speed > 3) { p.vx = (p.vx / speed) * 3; p.vy = (p.vy / speed) * 3 }

      p.x += p.vx * dt
      p.y += p.vy * dt
      p.phase += dt * 1.2

      const ix = Math.round(p.x), iy = Math.round(p.y)
      if (ix >= 0 && ix < this.w && iy >= 0 && iy < this.h) {
        const idx = iy * this.w + ix
        if (this.trail[idx] < 0.9) this.trail[idx] = 0.9
      }

      if (p.x < -2 || p.x > this.w + 2 || p.y < -2 || p.y > this.h + 2) {
        this.particles[i] = this.newParticle(false)
      }
    }
  }

  draw(buf) {
    const { palette, dim, bright } = this.theme
    for (let y = 0; y < this.h; y++) {
      for (let x = 0; x < this.w; x++) {
        const b = this.trail[y * this.w + x]
        if (b < 0.08) continue
        const pIdx = (x + y) % dim.length
        buf.set(x, y, '\u00b7', lerp(dim[pIdx], palette[pIdx], b * 0.45))
      }
    }
    for (const p of this.particles) {
      const x = Math.round(p.x), y = Math.round(p.y)
      if (x < 0 || x >= this.w || y < 0 || y >= this.h) continue
      const shimmer = 0.65 + 0.35 * Math.sin(p.phase)
      buf.set(x, y, p.glyph, lerp(palette[p.palIdx], bright, shimmer * 0.5))
    }
  }
}

const SCENES = { starfield: Starfield, rain: Rain, particles: Particles }

// ---------------------------------------------------------------------------
// Engine (port simplificado de internal/engine/engine.go): idle timer,
// ticker com dt limitado, ciclo de cenas com fade out/in.

let screen = null
let box = null
let hooks = {}
let opts = {}
let sceneList = []
let cur = 0
let buf = null
let active = false
let swallow = false
let ticker = null
let idleTimer = null
let lastActivity = Date.now()
let lastTick = 0
let sceneAge = 0
// transition: null | { phase: 'out'|'in', t: number }
let transition = null

const DEFAULTS = {
  enabled: true,
  idleSeconds: 90,
  fps: 20,
  cycleSeconds: 60,
  fadeSeconds: 0.6,
  theme: 'cosmic',
  scenes: 'all',
  wakeOnLog: true,
}

function buildScenes() {
  const spec = String(opts.scenes || 'all').trim().toLowerCase()
  let names = spec === '' || spec === 'all'
    ? Object.keys(SCENES)
    : spec.split(',').map(s => s.trim()).filter(s => SCENES[s])
  if (names.length === 0) names = Object.keys(SCENES)
  // shuffle (drift: Engine.Shuffle)
  for (let i = names.length - 1; i > 0; i--) {
    const j = randInt(i + 1)
    ;[names[i], names[j]] = [names[j], names[i]]
  }
  return names.map(n => new SCENES[n]())
}

function theme() {
  return THEMES[opts.theme] || THEMES.cosmic
}

function initScene() {
  const w = screen.width
  const h = screen.height
  buf = new Buffer(w, h)
  sceneList[cur].init(w, h, theme())
}

function fadeAlpha() {
  const dur = opts.fadeSeconds
  if (!transition || dur <= 0) return 0
  return transition.phase === 'out' ? transition.t / dur : 1 - transition.t / dur
}

function frame() {
  const now = Date.now()
  let dt = (now - lastTick) / 1000
  lastTick = now
  if (dt > 0.1) dt = 0.1

  if (transition) {
    transition.t += dt
    if (transition.t >= opts.fadeSeconds) {
      if (transition.phase === 'out') {
        cur = (cur + 1) % sceneList.length
        sceneAge = 0
        initScene()
        transition = { phase: 'in', t: 0 }
      } else {
        transition = null
      }
    }
  }

  const scene = sceneList[cur]
  scene.update(dt)
  buf.clear()
  scene.draw(buf)
  box.setContent(buf.toContent(fadeAlpha()))
  screen.render()

  if (opts.cycleSeconds > 0 && sceneList.length > 1 && !transition) {
    sceneAge += dt
    if (sceneAge >= opts.cycleSeconds) {
      if (opts.fadeSeconds > 0) transition = { phase: 'out', t: 0 }
      else {
        cur = (cur + 1) % sceneList.length
        sceneAge = 0
        initScene()
      }
    }
  }
}

function start(sceneName) {
  if (!screen || active) return
  if (sceneName && SCENES[sceneName]) {
    sceneList = [new SCENES[sceneName]()]
    cur = 0
  } else if (sceneList.length === 0 || sceneList.length === 1) {
    sceneList = buildScenes()
    cur = 0
  }
  active = true
  sceneAge = 0
  transition = null
  initScene()
  box.show()
  box.setFront()
  box.focus()
  lastTick = Date.now()
  ticker = setInterval(frame, Math.round(1000 / opts.fps))
  // O ENTER que submeteu o comando "saver" ainda propaga pelo blessed
  // depois deste start() (mesmo tick) — sem engolir, o próprio guard de
  // keypress desligaria o saver imediatamente.
  swallowTick()
}

function stop() {
  if (!active) return
  active = false
  if (ticker) { clearInterval(ticker); ticker = null }
  box.hide()
  if (hooks.onStop) hooks.onStop()
  screen.render()
}

// Registra atividade (tecla ou log). Se o saver está ativo, encerra.
function touch() {
  lastActivity = Date.now()
  if (active) stop()
}

// Log novo no proxy: acorda o saver (a menos que wakeOnLog esteja off).
function logActivity() {
  if (opts.wakeOnLog) touch()
}

function swallowTick() {
  swallow = true
  setImmediate(() => { swallow = false })
}

// Chamado no início de cada handler de tecla da TUI. Retorna true quando a
// tecla deve ser engolida (foi usada apenas para acordar o saver) — mesmo
// truque do drift, onde o primeiro keypress só sai da animação.
function handleKey() {
  lastActivity = Date.now()
  if (swallow) return true
  if (active) {
    stop()
    swallowTick()
    return true
  }
  return false
}

function checkIdle() {
  if (active || !opts.enabled) return
  const ok = !hooks.canStart || hooks.canStart()
  if (Date.now() - lastActivity >= opts.idleSeconds * 1000 && ok) start()
}

function init(scr, config, h) {
  screen = scr
  opts = { ...DEFAULTS, ...(config || {}) }
  hooks = h || {}

  box = blessed.box({
    parent: screen,
    left: 0, top: 0, width: '100%', height: '100%',
    style: { fg: 'white', bg: 'black' },
    hidden: true,
  })

  sceneList = buildScenes()

  screen.on('resize', () => {
    if (active) initScene()
  })

  idleTimer = setInterval(checkIdle, 1000)
  if (idleTimer.unref) idleTimer.unref()
}

function isActive() {
  return active
}

function sceneNames() {
  return Object.keys(SCENES)
}

module.exports = { init, start, stop, touch, logActivity, handleKey, isActive, sceneNames, THEMES }
