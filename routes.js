const fs = require('fs')
const path = require('path')

const STATIC_PATH = path.join(__dirname, 'routes.json')
const EXAMPLE_PATH = path.join(__dirname, 'routes.example.json')
const DYNAMIC_PATH = path.join(__dirname, 'routes-dynamic.json')

function resolveStaticPath() {
  if (fs.existsSync(STATIC_PATH)) return STATIC_PATH
  if (fs.existsSync(EXAMPLE_PATH)) return EXAMPLE_PATH
  return null
}

function loadFile(filePath) {
  try {
    if (filePath && fs.existsSync(filePath)) {
      return JSON.parse(fs.readFileSync(filePath, 'utf-8'))
    }
  } catch {}
  return []
}

function saveDynamic(routes) {
  fs.writeFileSync(DYNAMIC_PATH, JSON.stringify(routes, null, 2), 'utf-8')
}

function getRoutes() {
  return [...loadFile(resolveStaticPath()), ...loadFile(DYNAMIC_PATH)]
}

function addRoute(prefix, target, label) {
  if (!prefix.startsWith('/')) prefix = '/' + prefix
  const dynamic = loadFile(DYNAMIC_PATH)
  const all = getRoutes()
  if (all.find(r => r.prefix === prefix)) {
    return { ok: false, error: `Rota "${prefix}" j\u00E1 existe` }
  }
  dynamic.push({ prefix, target, label })
  saveDynamic(dynamic)
  return { ok: true, route: { prefix, target, label } }
}

function removeRoute(prefix) {
  const staticPath = resolveStaticPath()
  const staticRoutes = loadFile(staticPath)
  if (staticRoutes.find(r => r.prefix === prefix)) {
    return { ok: false, error: `Rota "${prefix}" \u00E9 fixa, n\u00E3o pode ser removida` }
  }
  const dynamic = loadFile(DYNAMIC_PATH)
  const idx = dynamic.findIndex(r => r.prefix === prefix)
  if (idx === -1) {
    return { ok: false, error: `Rota "${prefix}" n\u00E3o encontrada` }
  }
  dynamic.splice(idx, 1)
  saveDynamic(dynamic)
  return { ok: true }
}

module.exports = { getRoutes, addRoute, removeRoute }
