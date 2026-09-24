// Checks a packaged release the way the in-app updater will, before it's
// uploaded: the zip's signature must verify against the public key compiled
// into the app, and the zip must hold only what a release should (no data
// folder with someone's settings or sign-ins in it).
//
// Runs automatically at the end of `npm run tauri:portable`; run it by hand
// with `npm run verify:release [path/to/zip]`.

import { execFileSync } from 'node:child_process'
import crypto from 'node:crypto'
import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const zipPath = path.resolve(process.argv[2] ?? path.join(rootDir, 'release', 'portable', 'AppleCrap.zip'))
const failures = []

// The key the app trusts, read from the source so the two can't drift.
const updaterSource = await fs.readFile(path.join(rootDir, 'src-tauri', 'src', 'services', 'updater.rs'), 'utf8')
const keyBlock = updaterSource.match(/RELEASE_PUBLIC_KEY: \[u8; 32\] = \[([^\]]+)\]/)
if (!keyBlock) {
  throw new Error('Could not find RELEASE_PUBLIC_KEY in updater.rs')
}
const keyBytes = Buffer.from(keyBlock[1].match(/0x[0-9a-f]{2}/gi).map((byte) => Number.parseInt(byte, 16)))
// An Ed25519 public key in SPKI form is a fixed 12-byte prefix plus the key.
const publicKey = crypto.createPublicKey({
  key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), keyBytes]),
  format: 'der',
  type: 'spki',
})

const zip = await fs.readFile(zipPath)
let signature
try {
  signature = Buffer.from((await fs.readFile(`${zipPath}.sig`, 'utf8')).trim(), 'base64')
} catch {
  failures.push(`no signature file next to the zip (${path.basename(zipPath)}.sig)`)
}
if (signature && !crypto.verify(null, zip, publicKey, signature)) {
  failures.push("the signature doesn't match the key the app trusts")
}

// List the zip's contents with .NET, which every Windows machine has.
const listing = execFileSync(
  'powershell.exe',
  [
    '-NoProfile',
    '-Command',
    `Add-Type -AssemblyName System.IO.Compression.FileSystem; ` +
      `[System.IO.Compression.ZipFile]::OpenRead('${zipPath.replace(/'/g, "''")}').Entries | ForEach-Object { $_.FullName }`,
  ],
  { encoding: 'utf8' },
)
  .split(/\r?\n/)
  .map((line) => line.trim())
  .filter(Boolean)

const executables = listing.filter((name) => name.toLowerCase().endsWith('.exe'))
if (executables.length !== 1) {
  failures.push(`expected exactly one .exe in the zip, found ${executables.length}`)
}
const unexpected = listing.filter(
  (name) => !name.toLowerCase().endsWith('.exe') && !name.toLowerCase().endsWith('readme.txt') && !name.endsWith('/'),
)
if (unexpected.length) {
  failures.push(`unexpected files in the zip: ${unexpected.join(', ')}`)
}
if (listing.some((name) => /(^|\/)data\/.+/i.test(name))) {
  failures.push('the zip contains a data folder with files in it')
}

if (failures.length) {
  console.error(`\n*** RELEASE CHECK FAILED for ${zipPath}:`)
  for (const failure of failures) {
    console.error(`***  - ${failure}`)
  }
  process.exit(1)
}
console.log(`Release check passed: signature verifies, contents are ${listing.join(', ')}.`)
