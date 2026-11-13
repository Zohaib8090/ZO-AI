// Generates launcher icons (legacy + adaptive foreground), the splash icon and the
// notification glyph from crates/zohara-chat/data/zohara-chat.svg. Output is committed, so this
// only needs re-running when the logo changes:  npm run assets
import sharp from 'sharp';
import { readFileSync, writeFileSync, mkdirSync, rmSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const res = join(here, '..', 'android', 'app', 'src', 'main', 'res');
const src = readFileSync(join(here, '..', '..', '..', 'crates', 'zohara-chat', 'data', 'zohara-chat.svg'), 'utf8');
const BG = '#191919';

// The mark alone (the 120x120 group of the source svg), transparent background.
const markInner = src.match(/<g transform[^>]*>([\s\S]*?)<\/g>/)[1];
const markSvg = (size, fillOverride) => {
  let inner = markInner;
  if (fillOverride) inner = inner.replace(/(fill|stroke)="#(?!fff|000)[0-9A-Fa-f]{3,6}"/g, `$1="${fillOverride}"`);
  return Buffer.from(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120" width="${size}" height="${size}">${inner}</svg>`);
};
const dens = { mdpi: 1, hdpi: 1.5, xhdpi: 2, xxhdpi: 3, xxxhdpi: 4 };
const out = (dir, name, buf) => { mkdirSync(join(res, dir), { recursive: true }); writeFileSync(join(res, dir, name), buf); };

for (const [d, m] of Object.entries(dens)) {
  // Adaptive foreground: 108dp canvas, mark inside the 66dp safe zone (use 60dp).
  const fgSize = Math.round(108 * m), mark = Math.round(60 * m);
  const fg = await sharp({ create: { width: fgSize, height: fgSize, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } } })
    .composite([{ input: await sharp(markSvg(mark * 4)).resize(mark, mark).png().toBuffer(), gravity: 'center' }]).png().toBuffer();
  out(`mipmap-${d}`, 'ic_launcher_foreground.png', fg);
  // Legacy square + round icon on the dark tile (48dp).
  const s = Math.round(48 * m), inner = Math.round(34 * m);
  const markPng = await sharp(markSvg(inner * 4)).resize(inner, inner).png().toBuffer();
  const tile = (shape) => sharp({ create: { width: s, height: s, channels: 4, background: BG } })
    .composite([{ input: markPng, gravity: 'center' }, ...(shape ? [{ input: Buffer.from(`<svg width="${s}" height="${s}"><circle cx="${s / 2}" cy="${s / 2}" r="${s / 2}"/></svg>`), blend: 'dest-in' }] : [])]).png().toBuffer();
  out(`mipmap-${d}`, 'ic_launcher.png', await tile(false));
  out(`mipmap-${d}`, 'ic_launcher_round.png', await tile(true));
  // Notification small icon: white silhouette, 24dp.
  const n = Math.round(24 * m);
  out(`drawable-${d}`, 'ic_stat_zoai.png', await sharp(markSvg(n * 4, '#FFFFFF')).resize(n, n).png().toBuffer());
}
// Splash icon (shown centred on the dark background): 288dp-ish single density.
out('drawable-nodpi', 'splash_icon.png', await sharp(markSvg(1152)).resize(576, 576).png().toBuffer());
// Drop the stock Capacitor splash bitmaps; splash.xml replaces them.
for (const d of readdirSync(res).filter((x) => x.startsWith('drawable') && x !== 'drawable')) {
  try { rmSync(join(res, d, 'splash.png')); } catch {}
}
try { rmSync(join(res, 'drawable', 'splash.png')); } catch {}
// Adaptive icon definitions
out('mipmap-anydpi-v26', 'ic_launcher.xml', Buffer.from(`<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@color/ic_launcher_background"/>
    <foreground android:drawable="@mipmap/ic_launcher_foreground"/>
    <monochrome android:drawable="@mipmap/ic_launcher_foreground"/>
</adaptive-icon>
`));
out('mipmap-anydpi-v26', 'ic_launcher_round.xml', Buffer.from(`<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@color/ic_launcher_background"/>
    <foreground android:drawable="@mipmap/ic_launcher_foreground"/>
    <monochrome android:drawable="@mipmap/ic_launcher_foreground"/>
</adaptive-icon>
`));
console.log('assets written to', res);
