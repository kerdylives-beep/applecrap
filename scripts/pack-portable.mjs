import { execFile } from 'node:child_process'
import crypto from 'node:crypto'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const rootDir = path.resolve(__dirname, '..')
const targetReleaseDir = path.join(rootDir, 'src-tauri', 'target', 'release')
const outputRoot = path.join(rootDir, 'release', 'portable')
// The folder and exe names only matter for new downloads: an update keeps
// whatever exe name the install already has.
const portableDir = path.join(outputRoot, 'AppleCrap')
const portableZip = path.join(outputRoot, 'AppleCrap.zip')

const releaseEntries = await fs.readdir(targetReleaseDir, { withFileTypes: true })
const executables = releaseEntries
  .filter((entry) => entry.isFile() && entry.name.toLowerCase().endsWith('.exe'))
  .map((entry) => entry.name)
  .filter((name) => !name.toLowerCase().includes('uninstall'))

if (!executables.length) {
  throw new Error(`No built Tauri executable was found in ${targetReleaseDir}. Run "npm run tauri:build" first.`)
}

const sourceExecutable = path.join(targetReleaseDir, executables[0])
await fs.rm(portableDir, { recursive: true, force: true })
await fs.mkdir(path.join(portableDir, 'data'), { recursive: true })
await fs.copyFile(sourceExecutable, path.join(portableDir, 'AppleCrap.exe'))
await fs.writeFile(path.join(portableDir, 'README.txt'), buildPortableReadme(), 'utf8')

await compressDirectory(portableDir, portableZip)
console.log(`Portable build created:\n- ${portableDir}\n- ${portableZip}`)
const signed = await signRelease(portableZip)
if (signed) {
  // Check the finished zip exactly as the in-app updater will.
  await import('./verify-release.mjs')
}

/**
 * Writes `<zip>.sig`: a base64 ed25519 signature over the zip. The in-app
 * updater refuses any release whose zip does not verify against the public
 * key built into the app, so a release uploaded without this file can only
 * be installed by hand.
 *
 * The private key lives outside the repository (APPLECRAP_SIGNING_KEY, or
 * ~/.applecrap/release-signing-key.pem). Losing it means installed copies
 * cannot auto-update to anything signed with a new key.
 */
async function signRelease(zipPath) {
  const signaturePath = `${zipPath}.sig`
  // Never leave a signature from a previous build next to a new zip.
  await fs.rm(signaturePath, { force: true })

  const keyPath =
    process.env.APPLECRAP_SIGNING_KEY || path.join(os.homedir(), '.applecrap', 'release-signing-key.pem')
  let keyPem
  try {
    keyPem = await fs.readFile(keyPath)
  } catch {
    console.warn(
      `\n*** UNSIGNED BUILD: no signing key at ${keyPath}.\n` +
        '*** The in-app updater will not install this release automatically.\n',
    )
    return false
  }

  const signature = crypto.sign(null, await fs.readFile(zipPath), crypto.createPrivateKey(keyPem))
  await fs.writeFile(signaturePath, signature.toString('base64'), 'utf8')
  console.log(`- ${signaturePath} (signed)`)
  return true
}

function buildPortableReadme() {
  return [
    'AppleCrap',
    '=========',
    '',
    'Twitch song requests, played through Apple Music.',
    '',
    'Getting started:',
    '1. Unzip this folder anywhere you can write files (not Program Files).',
    '2. Run "AppleCrap.exe". If Windows says it protected your PC, click',
    '   "More info", then "Run anyway". This only happens the first time.',
    '3. Follow the three setup steps: sign in to Apple Music, sign in with',
    '   Twitch, connect to chat.',
    '',
    'Your settings and queue live in the "data" folder next to the app.',
    'Updates install themselves from the banner at the top of the app.',
    '',
    'You need Windows with WebView2 (already on Windows 10 and 11) and an',
    'Apple Music subscription. No Apple software needs to be installed.',
    '',
    'Something wrong? Help > Report a problem saves a report and opens an',
    'email to send it with.',
    '',
  ].join('\n')
}

function compressDirectory(sourceDir, outputZip) {
  // Paths are inlined because arguments after -Command are not bound to a
  // scriptblock's param() block. $ErrorActionPreference makes Compress-Archive
  // failures terminating so a bad run cannot exit 0 and leave a stale zip.
  const command = [
    "$ErrorActionPreference = 'Stop'",
    `if (Test-Path -LiteralPath '${outputZip}') { Remove-Item -LiteralPath '${outputZip}' -Force }`,
    `Compress-Archive -Path '${sourceDir}\\*' -DestinationPath '${outputZip}' -Force`,
  ].join('; ')

  return new Promise((resolve, reject) => {
    execFile(
      'powershell.exe',
      ['-NoProfile', '-NonInteractive', '-Command', command],
      (error, stdout, stderr) => {
        if (error) {
          reject(new Error(stderr || stdout || error.message))
          return
        }
        resolve()
      },
    )
  })
}
