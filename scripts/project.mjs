import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, readdirSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

export const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = (file) => readFileSync(join(root, file), 'utf8');
const json = (file) => JSON.parse(read(file));
const fail = (message) => {
  throw new Error(message);
};
export function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: 'inherit',
    shell: process.platform === 'win32',
    ...options,
  });
  if (result.error || result.status !== 0)
    fail(
      `${command} failed${result.error ? `: ${result.error.message}` : ` (exit ${result.status})`}`,
    );
}
const allowed = {
  web: ['dist', 'node_modules/.cache', 'playwright-report', 'test-results'],
  rust: ['src-tauri/target'],
  android: [
    'src-tauri/gen/android/app/build',
    'src-tauri/gen/android/build',
    'src-tauri/gen/android/.gradle',
  ],
};
export function clean(groups) {
  for (const group of groups) {
    if (!(group in allowed)) fail('Unknown cleanup group.');
    for (const path of allowed[group]) {
      const target = resolve(root, path);
      if (!target.startsWith(`${root}${sep}`)) fail('Cleanup path escaped the repository.');
      // Check every ancestor: never follow a generated output symlink into user data.
      let cursor = root;
      for (const part of relative(root, target).split(sep)) {
        cursor = join(cursor, part);
        if (existsSync(cursor) && lstatSync(cursor).isSymbolicLink())
          fail(`Refusing symlink cleanup: ${path}`);
      }
      if (existsSync(target)) {
        if (!realpathSync(target).startsWith(`${realpathSync(root)}${sep}`))
          fail('Cleanup target escaped the repository.');
        rmSync(target, { recursive: true, force: true });
      }
      console.log(`Clean: ${path}`);
    }
  }
}
export function verifyLocks() {
  for (const file of ['package-lock.json', 'src-tauri/Cargo.lock'])
    if (!existsSync(join(root, file)))
      fail(
        `${file} is missing. Run npm run bootstrap on a networked build machine, review and commit the generated locks.`,
      );
  const pkg = json('package.json');
  for (const [name, version] of Object.entries({ ...pkg.dependencies, ...pkg.devDependencies })) {
    if (!/^\d+\.\d+\.\d+$/.test(version)) fail(`${name} is not pinned to a stable exact version.`);
  }
  const lock = json('package-lock.json');
  if (lock.lockfileVersion < 3 || lock.packages?.['']?.version !== pkg.version)
    fail('The npm lockfile is invalid or stale.');
  for (const group of ['dependencies', 'devDependencies']) {
    const ordered = (value) =>
      JSON.stringify(Object.entries(value || {}).sort(([a], [b]) => a.localeCompare(b)));
    if (ordered(lock.packages?.['']?.[group]) !== ordered(pkg[group]))
      fail(`The lockfile ${group} do not match package.json.`);
  }
  if (!/^channel = "\d+\.\d+\.\d+"$/m.test(read('rust-toolchain.toml')))
    fail('Pin the Rust toolchain with npm run bootstrap.');
  console.log(
    'Dependency pins and lockfile presence: OK. Use npm ci and Cargo --locked for graph verification.',
  );
}
export function verifySource() {
  const pkg = json('package.json');
  const config = json('src-tauri/tauri.conf.json');
  if (pkg.name !== 'iptv' || pkg.version !== '1.0.0' || !pkg.private)
    fail('Unexpected npm project identity.');
  if (
    config.productName !== 'IPTV' ||
    config.version !== '1.0.0' ||
    config.identifier !== 'com.devsolux.iptv'
  )
    fail('Unexpected native application identity.');
  for (const file of [
    'src-tauri/Cargo.toml',
    'src-tauri/crates/iptv-core/Cargo.toml',
    'src-tauri/plugins/player/Cargo.toml',
  ])
    if (!read(file).includes('version = "1.0.0"')) fail(`Version mismatch in ${file}`);
  const forbidden =
    /capacitor|cordova|redux|workbox|vite-plugin-pwa|react-router-dom|@ionic\/react-router|video\.js/;
  for (const name of Object.keys({ ...pkg.dependencies, ...pkg.devDependencies }))
    if (forbidden.test(name)) fail(`Legacy or redundant dependency: ${name}`);
  for (const file of [
    'capacitor.config.ts',
    'netlify.toml',
    'playlist.js',
    'src/service-worker.ts',
  ])
    if (existsSync(join(root, file))) fail(`Legacy file remains: ${file}`);
  if (config.app.security.dangerousDisableAssetCspModification) fail('CSP must not be disabled.');
  const capability = json('src-tauri/capabilities/main.json');
  if (capability.remote || capability.permissions.some((p) => /^shell:|^sql:|^http:|^fs:/.test(p)))
    fail('Frontend native authority is broader than the application command boundary.');
  const commandList = [...read('src-tauri/build.rs').matchAll(/"([a-z][a-z_]+)"/g)].map(
    (m) => m[1],
  );
  const permission = read('src-tauri/permissions/application.toml');
  for (const command of commandList)
    if (!permission.includes(`"${command}"`))
      fail(`Command lacks application permission: ${command}`);
  for (const file of [
    'Makefile',
    'src/main.tsx',
    'src-tauri/src/main.rs',
    'public/icon.png',
    'docs/VERIFICATION.md',
  ])
    if (!existsSync(join(root, file))) fail(`Missing required file: ${file}`);
  const walk = (directory) =>
    readdirSync(directory, { withFileTypes: true }).flatMap((entry) =>
      entry.isDirectory() ? walk(join(directory, entry.name)) : [join(directory, entry.name)],
    );
  for (const file of walk(join(root, 'src'))) {
    if (!/\.(ts|tsx)$/.test(file)) continue;
    const text = readFileSync(file, 'utf8');
    if (/dangerouslySetInnerHTML|eval\s*\(|atob\s*\(|btoa\s*\(/.test(text))
      fail(`Unsafe or legacy frontend operation: ${relative(root, file)}`);
  }
  console.log('Project identity, declared boundaries, required assets and legacy cleanup: OK.');
}
function doctor() {
  let missing = false;
  console.log(`IPTV | com.devsolux.iptv | ${process.platform}/${process.arch}`);
  for (const [command, args, required] of [
    ['node', ['--version'], true],
    ['npm', ['--version'], true],
    ['cargo', ['--version'], true],
    ['rustc', ['--version'], true],
    ['java', ['-version'], false],
    ['adb', ['version'], false],
  ]) {
    const result = spawnSync(command, args, {
      cwd: root,
      encoding: 'utf8',
      shell: process.platform === 'win32',
      timeout: 10000,
    });
    console.log(
      `${command}: ${result.status === 0 ? (result.stdout || result.stderr).trim().split('\n')[0] : required ? 'MISSING (required)' : 'not found (Android only)'}`,
    );
    if (required && result.status !== 0) missing = true;
  }
  console.log(
    `Android SDK: ${process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT || 'not configured'}`,
  );
  console.log(`Android NDK: ${process.env.NDK_HOME || 'not configured'}`);
  console.log(
    `Dependency locks: ${existsSync(join(root, 'package-lock.json')) && existsSync(join(root, 'src-tauri/Cargo.lock')) ? 'present' : 'not generated; bootstrap is required'}`,
  );
  console.log(
    'Native WebView libraries, signing, codecs and physical-device behavior need platform verification. See docs/BUILDING.md.',
  );
  if (missing) process.exitCode = 1;
}
const help = `IPTV — com.devsolux.iptv

make doctor                 Inspect local tools; does not install anything
make bootstrap              Explicitly resolve current stable dependencies and create locks
make install                Install committed dependency graphs
make dev / dev-web          Native desktop app / read-only browser fixture preview
make android-init           Generate Tauri Android host; add scoped network/TV configuration
make dev-android            Run Android development session
make build / build-web      Current-host desktop package / frontend output
make build-android          Build ARMv7 + ARM64 Android packages (no publishing)
make build-macos-universal  Build both Mac architectures on macOS
make fmt / check            Apply formatting / run non-mutating verification
make test / test-rust       Frontend algorithm tests / Rust workspace tests
make test-sql / test-e2e    SQL invariants / browser integration tests
make verify-release        Check lockfiles and release metadata; not a certification
make clean[-web|-rust|-android]
                            Remove known outputs only; retain sources, locks and user data

Equivalent npm commands work without Make. Prerequisites and signing: docs/BUILDING.md.`;
// Node realpaths the ESM entry but not argv[1]; compare real paths so a symlinked checkout
// (for example macOS /var -> /private/var) runs the task instead of silently exiting 0.
const invoked = process.argv[1] && existsSync(process.argv[1]) ? realpathSync(process.argv[1]) : '';
if (invoked && invoked === realpathSync(fileURLToPath(import.meta.url))) {
  try {
    const task = process.argv[2] || 'help';
    if (task === 'help') console.log(help);
    else if (task === 'doctor') doctor();
    else if (task === 'verify') verifySource();
    else if (task === 'locks') verifyLocks();
    else if (task === 'release') {
      verifySource();
      verifyLocks();
      console.log(
        'Metadata gate passed. Run make check, native builds, device tests and the release checklist.',
      );
    } else if (task === 'macos') {
      if (process.platform !== 'darwin')
        fail('Universal macOS packaging requires a macOS build host.');
    } else if (task === 'clean') clean(['web', 'rust', 'android']);
    else if (task.startsWith('clean-')) clean([task.slice(6)]);
    else fail(`Unknown task: ${task}`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : 'Project task failed.');
    process.exitCode = 1;
  }
}
