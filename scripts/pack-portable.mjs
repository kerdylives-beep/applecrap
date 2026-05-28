import { execFile } from 'node:child_process'
import fs from 'node:fs/promises'
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
    '- Apple Music Windows app is required for playback handoff and probe testing.',
    '',
    'Diagnostics:',
    '- Use the in-app "Export diagnostics" action to create a support bundle.',
    '- The app keeps portable data in ./data when possible and falls back to Local AppData if the folder is not writable.',
    '',
  ].join('\n')
}

function compressDirectory(sourceDir, outputZip) {
  return new Promise((resolve, reject) => {
    execFile(
      'powershell.exe',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        `& { ${[
          'param($SourceDir, $OutputZip)',
          'if (Test-Path -LiteralPath $OutputZip) { Remove-Item -LiteralPath $OutputZip -Force }',
          'Get-ChildItem -LiteralPath $SourceDir | Compress-Archive -DestinationPath $OutputZip -Force',
        ].join('; ')} }`,
        sourceDir,
        outputZip,
      ],
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
