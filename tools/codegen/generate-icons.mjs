import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

// Generates the Windows application icon for the Tauri shell.
//
// apps/desktop/src-tauri/icons/icon.ico is required by tauri-build to generate the Windows resource
// file, so `cargo build --workspace` cannot complete without it. This writes a real multi-resolution .ico
// rather than a placeholder, and it is generated rather than committed as an opaque binary so the asset is
// reviewable and reproducible.
//
// Mayasaba is Windows-only (AGENTS.md section 3), so .ico is the required format and no .icns or Android
// mipmap set is produced. PNG renditions are also written because tauri.conf.json bundle.icon accepts them
// for installer assets.

const root = process.cwd();
const outDir = path.join(root, "apps/desktop/src-tauri/icons");

// Mayasaba's Control Room palette: a dark slate field with a lighter accent, matching the Control Room's
// minimal functional base (DEC-032).
const BG = [0x1b, 0x1f, 0x27];      // slate
const FG = [0x4f, 0x9d, 0xf7];      // accent
const EDGE = [0x2b, 0x33, 0x42];

const crcTable = (() => {
  const t = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c;
  }
  return t;
})();
const crc32 = (buf) => {
  let c = -1;
  for (let i = 0; i < buf.length; i++) c = crcTable[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
};
const png = (size) => {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8;      // bit depth
  ihdr[9] = 6;      // colour type RGBA
  const raw = Buffer.alloc(size * (size * 4 + 1));
  const r = Math.max(2, Math.round(size * 0.18));       // rounded-square radius
  const bar = Math.max(1, Math.round(size * 0.12));      // bento accent bar height
  for (let y = 0; y < size; y++) {
    const rowStart = y * (size * 4 + 1);
    raw[rowStart] = 0;                                   // filter: none
    for (let x = 0; x < size; x++) {
      const o = rowStart + 1 + x * 4;
      // rounded-square mask
      const dx = Math.max(r - x, 0, x - (size - 1 - r));
      const dy = Math.max(r - y, 0, y - (size - 1 - r));
      const inside = dx * dx + dy * dy <= r * r;
      if (!inside) {
        raw[o] = raw[o + 1] = raw[o + 2] = raw[o + 3] = 0;
        continue;
      }
      // bento accent: a horizontal band in the upper third
      const inBand = y > Math.round(size * 0.3) && y < Math.round(size * 0.3) + bar && x > Math.round(size * 0.22) && x < Math.round(size * 0.78);
      const edge = inBand && (y === Math.round(size * 0.3) + 1 || y === Math.round(size * 0.3) + bar - 2);
      const [cr, cg, cb] = inBand ? (edge ? EDGE : FG) : BG;
      raw[o] = cr;
      raw[o + 1] = cg;
      raw[o + 2] = cb;
      raw[o + 3] = 255;
    }
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", zlib.deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
};

// 32bpp BMP-in-ICO entry, which is what tauri-build's Windows resource generator consumes.
const bmpEntry = (size) => {
  const header = Buffer.alloc(40);
  header.writeUInt32LE(40, 0);
  header.writeInt32LE(size, 4);
  header.writeInt32LE(size * 2, 8);        // XOR + AND stacked
  header.writeUInt16LE(1, 12);
  header.writeUInt16LE(32, 14);
  header.writeUInt32LE(0, 16);             // BI_RGB
  const xor = Buffer.alloc(size * size * 4);
  const r = Math.max(2, Math.round(size * 0.18));
  const bar = Math.max(1, Math.round(size * 0.12));
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const dx = Math.max(r - x, 0, x - (size - 1 - r));
      const dy = Math.max(r - y, 0, y - (size - 1 - r));
      const inside = dx * dx + dy * dy <= r * r;
      const o = (y * size + x) * 4;
      if (!inside) continue;
      const inBand = y > Math.round(size * 0.3) && y < Math.round(size * 0.3) + bar && x > Math.round(size * 0.22) && x < Math.round(size * 0.78);
      const edge = inBand && (y === Math.round(size * 0.3) + 1 || y === Math.round(size * 0.3) + bar - 2);
      const [cr, cg, cb] = inBand ? (edge ? EDGE : FG) : BG;
      xor[o] = cb; xor[o + 1] = cg; xor[o + 2] = cr; xor[o + 3] = 255;
    }
  }
  const andRowBytes = Math.ceil(size / 32) * 4;
  const and = Buffer.alloc(andRowBytes * size); // all zero: alpha channel carries transparency
  const dib = Buffer.concat([header, xor, and]);
  return dib;
};

const SIZES = [16, 32, 48, 256];
const entries = SIZES.map((s) => bmpEntry(s));
const dir = Buffer.alloc(6 + 16 * SIZES.length);
dir.writeUInt16LE(0, 0);
dir.writeUInt16LE(1, 2);            // 1 = icon
dir.writeUInt16LE(SIZES.length, 4);
let offset = dir.length;
SIZES.forEach((s, i) => {
  const o = 6 + i * 16;
  dir[o] = s >= 256 ? 0 : s;        // 0 means 256
  dir[o + 1] = s >= 256 ? 0 : s;
  dir[o + 2] = 0;                   // palette size
  dir[o + 3] = 0;                   // reserved
  dir.writeUInt16LE(1, o + 4);      // colour planes
  dir.writeUInt16LE(32, o + 6);     // bits per pixel
  dir.writeUInt32LE(entries[i].length, o + 8);
  dir.writeUInt32LE(offset, o + 12);
  offset += entries[i].length;
});

fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, "icon.ico"), Buffer.concat([dir, ...entries]));
for (const s of [32, 128, 256]) {
  fs.writeFileSync(path.join(outDir, `${s}x${s}.png`), png(s));
}
console.log(
  `Generated icons/icon.ico (${SIZES.join("/")}) and 32x32.png, 128x128.png, 256x256.png`
);