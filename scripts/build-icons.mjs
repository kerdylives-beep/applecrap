import fs from 'node:fs/promises'
import path from 'node:path'
import sharp from 'sharp'

const root = process.cwd()
// The full icon, and a simpler one drawn for 32px and below.
const iconSvg = path.join(root, 'img', 'icon.svg')
const smallIconSvg = path.join(root, 'img', 'icon-small.svg')
const iconDir = path.join(root, 'build', 'icons')
const pngPath = path.join(iconDir, 'applecrap-icon.png')
const icoPath = path.join(iconDir, 'applecrap-icon.ico')

const ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
const SMALL_UP_TO = 32

await fs.mkdir(iconDir, { recursive: true })

const render = (svg, size) =>
  sharp(svg, { density: 300 }).resize(size, size).png().toBuffer()

await fs.writeFile(pngPath, await render(iconSvg, 1024))

const images = await Promise.all(
  ICO_SIZES.map((size) => render(size <= SMALL_UP_TO ? smallIconSvg : iconSvg, size)),
)
await fs.writeFile(icoPath, icoFromPngs(ICO_SIZES, images))

console.log(`Built icon assets:\n- ${pngPath}\n- ${icoPath}`)

/** An ICO file holding each size as a PNG (supported since Windows Vista). */
function icoFromPngs(sizes, pngs) {
  const header = Buffer.alloc(6)
  header.writeUInt16LE(0, 0)
  header.writeUInt16LE(1, 2)
  header.writeUInt16LE(pngs.length, 4)

  let offset = 6 + 16 * pngs.length
  const entries = pngs.map((png, index) => {
    const entry = Buffer.alloc(16)
    const size = sizes[index]
    entry.writeUInt8(size >= 256 ? 0 : size, 0)
    entry.writeUInt8(size >= 256 ? 0 : size, 1)
    entry.writeUInt16LE(1, 4)
    entry.writeUInt16LE(32, 6)
    entry.writeUInt32LE(png.length, 8)
    entry.writeUInt32LE(offset, 12)
    offset += png.length
    return entry
  })
  return Buffer.concat([header, ...entries, ...pngs])
}
