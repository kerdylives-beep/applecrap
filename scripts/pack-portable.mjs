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
const portableDir = path.join(outputRoot, 'AppleCrap Alpha')
const portableZip = path.join(outputRoot, 'AppleCrap Alpha.zip')

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
await fs.copyFile(sourceExecutable, path.join(portableDir, 'AppleCrap Alpha.exe'))
await fs.writeFile(path.join(portableDir, 'README.txt'), buildPortableReadme(), 'utf8')

await compressDirectory(portableDir, portableZip)
console.log(`Portable alpha created:\n- ${portableDir}\n- ${portableZip}`)
await signRelease(portableZip)

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
    return
  }

  const signature = crypto.sign(null, await fs.readFile(zipPath), crypto.createPrivateKey(keyPem))
  await fs.writeFile(signaturePath, signature.toString('base64'), 'utf8')
  console.log(`- ${signaturePath} (signed)`)
}

function buildPortableReadme() {
  return [
    'AppleCrap Alpha Portable',
    '========================',
    '',
    'What this is:',
    '- Windows-first Twitch to Apple Music request handoff desk.',
    '- Portable alpha build with local diagnostics and queue moderation.',
    '',
    'How to run:',
    '1. Unzip this folder anywhere you have write access.',
    '2. Launch "AppleCrap Alpha.exe".',
    '3. Keep the "data" folder beside the executable for portable storage.',
    '',
    'Important prerequisites:',
    '- WebView2 is required on Windows for Tauri apps.',
    '- A Twitch bot account token that starts with oauth: is required for chat connection.',
    '- An Apple Music subscription. Click "Player" inside the app and sign in once; no other Apple software is needed.',
    '',
    'Diagnostics:',
    '- Use the in-app "Export diagnostics" action to create a support bundle.',
    '- The app keeps portable data in ./data when possible and falls back to Local AppData if the folder is not writable.',
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
