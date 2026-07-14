const http = require('http')
const https = require('https')
const fs = require('fs')
const tui = require('./tui')
const { getRoutes, addRoute, removeRoute } = require('./routes')
const { shouldShow, handleCommand, filters, rebuild: rebuildFilters } = require('./filters')
const fileLogger = require('./fileLogger')
const { SERVICE_COLORS, statusColor, ANSI_RESET, ANSI_BOLD, ANSI_DIM, ANSI_RED, ANSI_CYAN, ANSI_GREEN, ANSI_YELLOW } = require('./colors')

const configPort = (() => {
  try { return require('./config.json').port } catch { return null }
})()
const PROXY_PORT = parseInt(process.env.PORT, 10) || configPort || 8888
const MAX_BODY_LOG = 1000
const MAX_LOG_BODY = 500
const MAX_RES_BUFFER = 10 * 1024 * 1024
const REQUEST_TIMEOUT = 120 * 1000
const BINARY_CONTENT_TYPES = ['image/', 'video/', 'audio/', 'application/octet-stream']

console.log = function (...args) {
  const text = args.map(a => (typeof a === 'string' ? a : JSON.stringify(a))).join(' ')
  tui.log(text)
}

function timestamp() {
  const d = new Date()
  return [String(d.getHours()).padStart(2, '0'), String(d.getMinutes()).padStart(2, '0'), String(d.getSeconds()).padStart(2, '0')].join(':')
}

function isBinary(contentType) {
  if (!contentType) return false
  return BINARY_CONTENT_TYPES.some(t => contentType.split(';')[0].toLowerCase().startsWith(t))
}

// S\u00f3 pra conveni\u00eancia de debug local \u2014 o proxy nunca roda em produ\u00e7\u00e3o, ent\u00e3o
// isso nunca decodifica nada fora da m\u00e1quina do dev.
const JWT_PATTERN = /\b[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/g

function decodeJwtPayload(token) {
  try {
    const payloadSegment = token.split('.')[1]
    let b64 = payloadSegment.replace(/-/g, '+').replace(/_/g, '/')
    while (b64.length % 4) b64 += '='
    return JSON.parse(Buffer.from(b64, 'base64').toString('utf-8'))
  } catch {
    return null
  }
}

function annotateJwts(fullText) {
  const tokens = [...new Set(fullText.match(JWT_PATTERN) || [])]
  const blocks = tokens
    .map(token => {
      const payload = decodeJwtPayload(token)
      if (!payload) return null
      return `  ${ANSI_DIM}[JWT ${token.slice(0, 16)}\u2026 decoded]${ANSI_RESET}\n  ${JSON.stringify(payload, null, 2).replace(/\n/g, '\n  ')}`
    })
    .filter(Boolean)
  return blocks.join('\n')
}

function formatBody(buf, contentType) {
  if (!buf || buf.length === 0) return ''
  if (isBinary(contentType)) return `  [binary ${buf.length.toLocaleString()} bytes]`

  const fullStr = buf.toString('utf-8')
  let pretty = fullStr
  try { pretty = JSON.stringify(JSON.parse(fullStr), null, 2) } catch {}

  const truncated = pretty.length > MAX_BODY_LOG ? pretty.slice(0, MAX_BODY_LOG) + '\u2026' : pretty
  const jwtAnnotations = annotateJwts(fullStr)

  return jwtAnnotations ? `${truncated}\n${jwtAnnotations}` : truncated
}

function findRoute(pathname) {
  const sorted = [...getRoutes()].sort((a, b) => b.prefix.length - a.prefix.length)
  for (const route of sorted) {
    if (pathname === route.prefix || pathname.startsWith(route.prefix + '/')) return route
  }
  return null
}

function collectBody(req) {
  return new Promise(resolve => {
    const chunks = []
    req.on('data', c => chunks.push(c))
    req.on('end', () => resolve(Buffer.concat(chunks)))
    req.on('error', () => resolve(Buffer.alloc(0)))
  })
}

function logRequest(reqId, method, url, targetFull, contentType, body, color, routeLabel) {
  if (!shouldShow(routeLabel)) return
  const c = color || ANSI_RESET
  console.log(`${ANSI_DIM}[${timestamp()}]${ANSI_RESET} ${ANSI_BOLD}${reqId}${ANSI_RESET}`)
  console.log(`  ${c}${method}${ANSI_RESET} ${url}`)
  console.log(`${ANSI_DIM}  \u2192 ${targetFull}${ANSI_RESET}`)
  if (body && body.length > 0) {
    console.log(`${ANSI_DIM}  Req Body (${(contentType || '').split(';')[0]}):${ANSI_RESET}`)
    console.log(`  ${formatBody(body, contentType).replace(/\n/g, '\n  ')}`)
  }
}

function logResponse(reqId, statusCode, headers, body, contentType, duration, color, routeLabel) {
  if (!shouldShow(routeLabel)) return
  const c = color || ANSI_RESET
  const sizeLabel = body && body.length > MAX_RES_BUFFER ? ` [${(body.length / 1024 / 1024).toFixed(1)}MB, omitted]` : ''
  console.log(`  ${ANSI_BOLD}Response:${ANSI_RESET} ${c}${statusCode}${ANSI_RESET} ${ANSI_DIM}${duration}ms${ANSI_RESET}${sizeLabel}`)
  if (body && body.length > 0 && body.length <= MAX_RES_BUFFER) {
    console.log(`${ANSI_DIM}  Res Body (${(contentType || '').split(';')[0]}):${ANSI_RESET}`)
    console.log(`  ${formatBody(body, contentType).replace(/\n/g, '\n  ')}`)
  }
  console.log('')
}

function sendError(res, status, message) {
  res.writeHead(status, { 'Content-Type': 'application/json' })
  res.end(JSON.stringify(message))
}

function forwardRequest(req, res, route, reqId, startTime) {
  const strippedPath = req.parsedUrl.pathname.slice(route.prefix.length) || '/'
  const targetPath = strippedPath + req.parsedUrl.search
  const routeColor = SERVICE_COLORS[route.label] || ANSI_RESET

  collectBody(req).then(requestBody => {
    logRequest(reqId, req.method, req.url, `${route.target}${targetPath}`, req.headers['content-type'], requestBody, routeColor, route.label)

    const backendUrl = new URL(route.target)
    const options = {
      hostname: backendUrl.hostname, port: backendUrl.port || 443,
      path: targetPath, method: req.method,
      headers: { ...req.headers, host: backendUrl.hostname },
      rejectUnauthorized: false, timeout: REQUEST_TIMEOUT,
    }
    delete options.headers['connection']
    delete options.headers['keep-alive']
    delete options.headers['transfer-encoding']

    const proxyReq = https.request(options, proxyRes => {
      const resChunks = []
      res.writeHead(proxyRes.statusCode, proxyRes.headers)
      proxyRes.on('data', chunk => { resChunks.push(chunk); res.write(chunk) })
      proxyRes.on('end', () => {
        const resBody = Buffer.concat(resChunks)
        const duration = Date.now() - startTime
        logResponse(reqId, proxyRes.statusCode, proxyRes.headers, resBody, proxyRes.headers['content-type'], duration, statusColor(proxyRes.statusCode), route.label)
        res.end()
      })
    })

    proxyReq.on('error', err => {
      console.log(`  ${ANSI_RED}ERROR:${ANSI_RESET} ${err.message} ${ANSI_DIM}${Date.now() - startTime}ms${ANSI_RESET}\n`)
      if (!res.headersSent) sendError(res, 502, { error: err.message })
    })
    proxyReq.on('timeout', () => {
      proxyReq.destroy()
      console.log(`  ${ANSI_RED}TIMEOUT${ANSI_RESET} ${ANSI_DIM}${Date.now() - startTime}ms${ANSI_RESET}\n`)
      if (!res.headersSent) sendError(res, 504, { error: 'Timeout (120s)' })
    })

    if (requestBody.length > 0) proxyReq.write(requestBody)
    proxyReq.end()
  })
}

function handleRequest(req, res) {
  const startTime = Date.now()
  const reqId = Math.random().toString(36).slice(2, 8)

  if (req.url === '/health') {
    res.writeHead(200, { 'Content-Type': 'application/json' })
    res.end(JSON.stringify({ status: 'ok', uptime: Math.floor(process.uptime()) }))
    return
  }

  if (req.url === '/log') {
    collectBody(req).then(body => {
      const raw = body.length > 0 ? body.toString('utf-8') : ''
      console.log(`${ANSI_DIM}[${timestamp()}]${ANSI_RESET} ${ANSI_CYAN}LOG${ANSI_RESET} ${raw.slice(0, MAX_LOG_BODY)}`)
      res.writeHead(200, { 'Content-Type': 'application/json' })
      res.end(JSON.stringify({ received: true }))
    })
    return
  }

  if (req.url === '/api/status') {
    res.writeHead(200, { 'Content-Type': 'application/json' })
    res.end(JSON.stringify({
      uptime: Math.floor(process.uptime()),
      port: PROXY_PORT,
      logFile: fileLogger.getSessionFile(),
      filters,
      routes: getRoutes().map(r => ({ prefix: r.prefix, target: r.target, label: r.label })),
    }, null, 2))
    return
  }

  if (req.url === '/api/logs') {
    const file = fileLogger.getSessionFile()
    const lines = 50
    let content = ''
    try {
      if (file && fs.existsSync(file)) {
        const raw = fs.readFileSync(file, 'utf-8')
        const all = raw.split('\n')
        content = all.slice(-lines - 1).join('\n')
      }
    } catch {}
    res.writeHead(200, { 'Content-Type': 'text/plain; charset=utf-8' })
    res.end(content || '(sem logs ainda)')
    return
  }

  if (req.method === 'POST' && req.url === '/api/cmd') {
    collectBody(req).then(body => {
      const { cmd } = JSON.parse(body.toString('utf-8') || '{}')
      if (!cmd) {
        sendError(res, 400, { error: 'Missing "cmd" field' })
        return
      }
      const parts = cmd.trim().split(/\s+/)
      const action = parts[0].toLowerCase()

      if (action === 'add' && parts.length >= 3) {
        const prefix = parts[1]
        const target = parts[2]
        const label = parts.slice(3).join(' ') || prefix.replace('/', '').toUpperCase()
        const result = addRoute(prefix, target, label)
        if (result.ok) {
          rebuildFilters()
          tui.updateSidebar()
          console.log(`${ANSI_GREEN}Route added:${ANSI_RESET} ${prefix} \u2192 ${target}`)
        }
        res.writeHead(result.ok ? 200 : 400, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify(result))
        return
      }

      if (action === 'rm' && parts.length >= 2) {
        const result = removeRoute(parts[1])
        if (result.ok) {
          rebuildFilters()
          tui.updateSidebar()
          console.log(`${ANSI_YELLOW}Route removed:${ANSI_RESET} ${parts[1]}`)
        }
        res.writeHead(result.ok ? 200 : 400, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify(result))
        return
      }

      if (action === 'logmode' && parts.length >= 2) {
        if (parts[1] === 'day' || parts[1] === 'session') {
          fileLogger.setMode(parts[1])
          tui.updateSidebar()
        }
        res.writeHead(200, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify({ mode: fileLogger.getMode(), file: fileLogger.getSessionFile() }))
        return
      }

      handleCommand(cmd)
      tui.updateSidebar()
      res.writeHead(200, { 'Content-Type': 'application/json' })
      res.end(JSON.stringify({ ok: true, cmd, filters }))
    }).catch(() => sendError(res, 400, { error: 'Invalid JSON body' }))
    return
  }

  try { req.parsedUrl = new URL(req.url, `http://localhost:${PROXY_PORT}`) }
  catch { sendError(res, 400, { error: 'URL inv\u00E1lida' }); return }

  const route = findRoute(req.parsedUrl.pathname)
  if (!route) {
    console.log(`${ANSI_DIM}[${timestamp()}]${ANSI_RESET} ${ANSI_BOLD}${reqId}${ANSI_RESET} \u26A0  NO ROUTE: ${req.method} ${req.url}`)
    sendError(res, 404, { error: 'No route', url: req.url, routes: getRoutes().map(r => `${r.prefix} \u2192 ${r.label}`) })
    return
  }

  forwardRequest(req, res, route, reqId, startTime)
}

const server = http.createServer(handleRequest)
server.on('error', err => {
  if (err.code === 'EADDRINUSE') {
    console.error(`Porta ${PROXY_PORT} em uso.\n`)
    process.exit(1)
  }
})

server.listen(PROXY_PORT, () => {
  const port = PROXY_PORT
  const routes = getRoutes()
  tui.init(port, routes)
})
