// 生成侧载清单需要的 16/32/80 像素图标（纯色方块 + 浅色书脊），不依赖图像库。
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

function crc32(buf: Buffer): number {
  let c = ~0;
  for (const b of buf) {
    c ^= b;
    for (let k = 0; k < 8; k++) c = c & 1 ? (c >>> 1) ^ 0xedb88320 : c >>> 1;
  }
  return ~c >>> 0;
}

function chunk(type: string, data: Buffer): Buffer {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}

function png(size: number): Buffer {
  const rows: Buffer[] = [];
  for (let y = 0; y < size; y++) {
    const row = Buffer.alloc(1 + size * 4);
    for (let x = 0; x < size; x++) {
      const spine = x > size * 0.3 && x < size * 0.42 && y > size * 0.2 && y < size * 0.8;
      const page = x >= size * 0.42 && x < size * 0.75 && y > size * 0.2 && y < size * 0.8;
      const [r, g, b] = spine ? [255, 255, 255] : page ? [210, 236, 232] : [22, 110, 102];
      row.set([r, g, b, 255], 1 + x * 4);
    }
    rows.push(row);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr.set([8, 6, 0, 0, 0], 8);
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(Buffer.concat(rows))),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

for (const s of [16, 32, 64, 80]) writeFileSync(`public/icons/icon-${s}.png`, png(s));
console.log("icons written");
