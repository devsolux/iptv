// Rebuilds every icon from src-tauri/icons/source. Run it only when the artwork changes and commit
// the results; builds never run it. Needs ImageMagick 7 (`magick`).
import { execFileSync } from 'node:child_process';
import { copyFileSync, cpSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from './project.mjs';

const CANVAS = 1024;
const icons = join(root, 'src-tauri/icons');
const symbol = join(icons, 'source/symbol.png');
const wordmark = join(icons, 'source/wordmark.png');
const backdrop = ['-define', 'gradient:angle=135', 'gradient:#1b2a45-#0a0f1a'];
// Symbol height over its tile, and the lift that centres its enclosing circle.
const SYMBOL_IN_TILE = 0.68;
const LIFT = 0.0141;
// Launchers may crop to any shape inside the 66dp circle of the 108dp canvas: the symbol's
// enclosing circle is 0.5751 of its height, so 530 of 1024 stays inside with a little slack.
const ANDROID_SYMBOL = 530;
// Apple's grid is an 824 tile on a 1024 canvas; elsewhere a margin only shrinks small sizes.
const MAC_TILE = 824;
const APP_TILE = 968;
const LAUNCHER_SIZES = { mdpi: 48, hdpi: 72, xhdpi: 96, xxhdpi: 144, xxxhdpi: 192 };

const magick = (...args) => execFileSync('magick', args, { stdio: 'inherit' });
const work = mkdtempSync(join(tmpdir(), 'iptv-icons-'));
const at = (name) => join(work, name);

function squircle(side, path) {
  const points = [];
  for (let step = 0; step < 720; step += 1) {
    const angle = (step * Math.PI) / 360;
    const axis = (value) => Math.sign(value) * Math.abs(value) ** (2 / 5);
    const x = CANVAS + side * axis(Math.cos(angle));
    const y = CANVAS + side * axis(Math.sin(angle));
    points.push(`${x.toFixed(2)},${y.toFixed(2)}`);
  }
  magick(
    '-size',
    `${CANVAS * 2}x${CANVAS * 2}`,
    'xc:black',
    '-fill',
    'white',
    '-draw',
    `polygon ${points.join(' ')}`,
    '-resize',
    `${CANVAS}x${CANVAS}`,
    path,
  );
}

const placed = (height) => [
  '(',
  symbol,
  '-resize',
  `x${height}`,
  ')',
  '-gravity',
  'center',
  '-geometry',
  `+0-${Math.round(height * LIFT)}`,
  '-composite',
];

function tile(side, path) {
  const mask = at(`mask-${side}.png`);
  squircle(side, mask);
  magick(
    '-size',
    `${CANVAS}x${CANVAS}`,
    ...backdrop,
    mask,
    '-alpha',
    'off',
    '-compose',
    'CopyOpacity',
    '-composite',
    '-compose',
    'Over',
    ...placed(Math.round(side * SYMBOL_IN_TILE)),
    // tauri icon resizes without premultiplying, so transparent pixels need a matching colour.
    '-background',
    '#0d1320',
    '-alpha',
    'background',
    `PNG32:${path}`,
  );
}

function macos(path) {
  const flat = at('mac-flat.png');
  const shadow = at('mac-shadow.png');
  tile(MAC_TILE, flat);
  magick(flat, '-alpha', 'extract', '-blur', '0x14', '-roll', '+0+12', shadow);
  magick(
    '-size',
    `${CANVAS}x${CANVAS}`,
    'xc:black',
    shadow,
    '-alpha',
    'off',
    '-compose',
    'CopyOpacity',
    '-composite',
    '-channel',
    'A',
    '-evaluate',
    'multiply',
    '0.32',
    '+channel',
    flat,
    '-compose',
    'Over',
    '-composite',
    `PNG32:${path}`,
  );
}

function androidLayers() {
  const foreground = at('foreground.png');
  const alpha = at('monochrome-alpha.png');
  magick('-size', `${CANVAS}x${CANVAS}`, ...backdrop, `PNG24:${at('background.png')}`);
  magick(
    '-size',
    `${CANVAS}x${CANVAS}`,
    'xc:none',
    ...placed(ANDROID_SYMBOL),
    '-background',
    '#4a6cf7',
    '-alpha',
    'background',
    `PNG32:${foreground}`,
  );
  // Themed icons keep only alpha, so the white play mark becomes a cut-out.
  magick(
    foreground,
    '(',
    '-clone',
    '0',
    '-alpha',
    'off',
    '-channel',
    'R',
    '-separate',
    '+channel',
    '-level',
    '70%,95%',
    '-negate',
    ')',
    '(',
    '-clone',
    '0',
    '-alpha',
    'extract',
    ')',
    '-delete',
    '0',
    '-compose',
    'Multiply',
    '-composite',
    alpha,
  );
  magick(
    '-size',
    `${CANVAS}x${CANVAS}`,
    'xc:white',
    alpha,
    '-alpha',
    'off',
    '-compose',
    'CopyOpacity',
    '-composite',
    `PNG32:${at('monochrome.png')}`,
  );
}

// tauri icon only cuts a circle out of the tile, which clips the symbol. Launchers crop the middle
// 72dp of an adaptive icon, so this builds the round icon the same way.
function roundLauncher(android) {
  const master = at('round.png');
  const mask = at('round-mask.png');
  const half = CANVAS / 2;
  magick(
    '-size',
    `${CANVAS}x${CANVAS}`,
    'xc:black',
    '-fill',
    'white',
    '-draw',
    `circle ${half},${half} ${half},2`,
    mask,
  );
  magick(
    at('background.png'),
    at('foreground.png'),
    '-composite',
    '-gravity',
    'center',
    '-crop',
    '683x683+0+0',
    '+repage',
    '-resize',
    `${CANVAS}x${CANVAS}`,
    mask,
    '-alpha',
    'off',
    '-compose',
    'CopyOpacity',
    '-composite',
    master,
  );
  for (const [density, side] of Object.entries(LAUNCHER_SIZES))
    magick(
      master,
      '-resize',
      `${side}x${side}`,
      `PNG32:${join(android, `mipmap-${density}/ic_launcher_round.png`)}`,
    );
}

function banner(path) {
  // Drawn at 3x and reduced: Android TV shows 320x180 and expects the name as text.
  const [width, height, symbolHeight, markWidth, gap] = [960, 540, 300, 400, 54];
  const symbolWidth = Math.round((symbolHeight * 910) / 1024);
  const left = Math.round((width - symbolWidth - gap - markWidth) / 2);
  magick(
    '-size',
    `${width}x${height}`,
    ...backdrop,
    '(',
    symbol,
    '-resize',
    `x${symbolHeight}`,
    ')',
    '-gravity',
    'west',
    '-geometry',
    `+${left}+0`,
    '-composite',
    '(',
    wordmark,
    '-resize',
    `${markWidth}x`,
    ')',
    '-gravity',
    'west',
    '-geometry',
    `+${left + symbolWidth + gap}+0`,
    '-composite',
    '-resize',
    '320x180',
    `PNG24:${path}`,
  );
}

const tauriIcon = (input, output) =>
  execFileSync('npx', ['--no-install', 'tauri', 'icon', input, '-o', output], {
    cwd: root,
    stdio: 'ignore',
  });

try {
  tile(APP_TILE, at('app.png'));
  macos(at('macos.png'));
  androidLayers();
  writeFileSync(
    at('icons.json'),
    JSON.stringify({
      default: 'app.png',
      android_bg: 'background.png',
      android_fg: 'foreground.png',
      android_fg_scale: 100,
      android_monochrome: 'monochrome.png',
    }),
  );
  tauriIcon(at('icons.json'), at('out'));
  tauriIcon(at('macos.png'), at('mac'));
  for (const name of ['32x32.png', '128x128.png', '128x128@2x.png', 'icon.ico'])
    copyFileSync(join(at('out'), name), join(icons, name));
  copyFileSync(join(at('mac'), 'icon.icns'), join(icons, 'icon.icns'));
  copyFileSync(join(at('out'), '128x128@2x.png'), join(root, 'public/icon.png'));
  rmSync(join(icons, 'android'), { recursive: true, force: true });
  cpSync(join(at('out'), 'android'), join(icons, 'android'), { recursive: true });
  roundLauncher(join(icons, 'android'));
  banner(join(icons, 'tv-banner.png'));
  console.log('Rebuilt desktop, Android, TV banner and web icons. Review them, then commit.');
} catch (error) {
  console.error(error instanceof Error ? error.message : 'Icon generation failed.');
  process.exitCode = 1;
} finally {
  rmSync(work, { recursive: true, force: true });
}
