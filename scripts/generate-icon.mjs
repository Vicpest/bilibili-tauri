import { mkdirSync, writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';

const size = 64;
const raw = Buffer.alloc((size * 4 + 1) * size);
for (let y = 0; y < size; y++) {
  for (let x = 0; x < size; x++) {
    const offset = y * (size * 4 + 1) + 1 + x * 4;
    const inside = x >= 8 && x < 56 && y >= 8 && y < 56;
    const play = x >= 25 && x <= 43 && Math.abs(y - 32) <= (x - 24) * 0.7;
    const color = play ? [255, 255, 255, 255] : inside ? [250, 114, 153, 255] : [0, 0, 0, 0];
    for (let i = 0; i < 4; i++) raw[offset + i] = color[i];
  }
}

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let i = 0; i < 8; i++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function chunk(name, data) {
  const type = Buffer.from(name);
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  let crc = 0xffffffff;
  for (const byte of Buffer.concat([type, data])) crc = crcTable[(crc ^ byte) & 255] ^ (crc >>> 8);
  const checksum = Buffer.alloc(4);
  checksum.writeUInt32BE((crc ^ 0xffffffff) >>> 0);
  return Buffer.concat([length, type, data, checksum]);
}
const header = Buffer.alloc(13);
header.writeUInt32BE(size, 0);
header.writeUInt32BE(size, 4);
header[8] = 8;
header[9] = 6;
const png = Buffer.concat([
  Buffer.from('89504e470d0a1a0a', 'hex'),
  chunk('IHDR', header),
  chunk('IDAT', deflateSync(raw)),
  chunk('IEND', Buffer.alloc(0)),
]);
const icoHeader = Buffer.from('0000010001004040000001002000', 'hex');
const directory = Buffer.alloc(8);
directory.writeUInt32LE(png.length, 0);
directory.writeUInt32LE(22, 4);
mkdirSync('src-tauri/icons', { recursive: true });
writeFileSync('src-tauri/icons/icon.ico', Buffer.concat([icoHeader, directory, png]));
