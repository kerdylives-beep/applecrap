import { spawn } from 'node:child_process'
import path from 'node:path'

const rootDir = path.resolve(import.meta.dirname, '..')
const viteEntrypoint = path.join(rootDir, 'node_modules', 'vite', 'bin', 'vite.js')
const electronExecutable = path.join(rootDir, 'node_modules', 'electron', 'dist', 'electron.exe')

let shuttingDown = false
let electronProcess = null
let bufferedOutput = ''

function shutdown(code = 0) {
  if (shuttingDown) {
    return
  }

  shuttingDown = true

  if (electronProcess && !electronProcess.killed) {
    electronProcess.kill()
  }

  if (!viteProcess.killed) {
    viteProcess.kill()
  }

  process.exit(code)
}

function startElectron(devUrl) {
  if (electronProcess) {
    return
  }

  console.log(`Starting Electron with ${devUrl}`)

  electronProcess = spawn(electronExecutable, ['.'], {
    stdio: 'inherit',
    cwd: rootDir,
    env: {
      ...process.env,
      VITE_DEV_SERVER_URL: devUrl,
    },
  })

  electronProcess.on('exit', (code) => {
    shutdown(code ?? 0)
  })

  electronProcess.on('error', (error) => {
    console.error(`Electron failed to start: ${error.stack || error}`)
    shutdown(1)
  })
}

const viteProcess = spawn(process.execPath, [viteEntrypoint, '--host', '127.0.0.1', '--port', '5173'], {
  stdio: ['inherit', 'pipe', 'pipe'],
  cwd: rootDir,
  env: process.env,
})

viteProcess.stdout.on('data', (chunk) => {
  const text = chunk.toString()
  bufferedOutput += text
  process.stdout.write(text)

  const match = bufferedOutput.match(/(http:\/\/127\.0\.0\.1:\d+)/)
  if (match) {
    startElectron(match[1])
    return
  }

  if (bufferedOutput.includes('ready in') || bufferedOutput.includes('Local:')) {
    startElectron('http://127.0.0.1:5173')
  }
})

viteProcess.stderr.on('data', (chunk) => {
  process.stderr.write(chunk.toString())
})

viteProcess.on('exit', (code) => {
  if (!electronProcess) {
    process.exit(code ?? 1)
    return
  }

  shutdown(code ?? 0)
})

process.on('SIGINT', () => shutdown(0))
process.on('SIGTERM', () => shutdown(0))
