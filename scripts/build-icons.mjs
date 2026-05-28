import fs from 'node:fs/promises'
import path from 'node:path'
import sharp from 'sharp'

const root = process.cwd()
const sourceSvg = path.join(root, 'public', 'applecrap-icon.svg')
const sourceIco = path.join(root, 'img', 'favicon.ico')
const iconDir = path.join(root, 'build', 'icons')
const pngPath = path.join(iconDir, 'applecrap-icon.png')
const icoPath = path.join(iconDir, 'applecrap-icon.ico')
const customIcoPath = path.join(root, 'build', 'icons', 'applecrap-icon.custom.ico')

await fs.mkdir(iconDir, { recursive: true })

let pngBuffer

try {
  pngBuffer = await sharp(sourceIco)
    .resize(1024, 1024)
    .png()
    .toBuffer()
} catch {
  pngBuffer = await sharp(sourceSvg)
    .resize(1024, 1024)
    .png()
    .toBuffer()
}

await fs.writeFile(pngPath, pngBuffer)

try {
  await fs.access(customIcoPath)
  await fs.copyFile(customIcoPath, icoPath)
} catch {
  try {
    await fs.copyFile(sourceIco, icoPath)
  } catch (error) {
    throw new Error(`No ICO source was available at ${sourceIco}: ${error.message}`)
  }
}

console.log(`Built icon assets:\n- ${pngPath}\n- ${icoPath}`)
