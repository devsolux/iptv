/** Explicit dependency maintenance. Ordinary builds never perform upgrades. */
import { readFile, writeFile, rename } from 'node:fs/promises';
import { join } from 'node:path';
import { root, run } from './project.mjs';
const stable = /^\d+\.\d+\.\d+$/;
async function download(url, json = true) {
  const response = await fetch(url, { headers: { 'User-Agent': 'IPTV-build-bootstrap/1.0.0' }, signal: AbortSignal.timeout(30000) });
  if (!response.ok) throw new Error(`Dependency metadata request failed (${response.status}): ${new URL(url).hostname}`);
  return json ? response.json() : response.text();
}
async function writeAtomic(path, content) { await writeFile(`${path}.tmp`, content); await rename(`${path}.tmp`, path); }
async function main() {
  // Preflight before changing manifests. No sudo, global installations or credential prompts.
  run('npm', ['--version']); run('cargo', ['--version']);
  const pkgPath = join(root, 'package.json');
  const pkg = JSON.parse(await readFile(pkgPath, 'utf8'));
  const report = { checkedAt: new Date().toISOString(), npm: {}, crates: {}, rust: '', media3: '' };
  const groups = ['dependencies', 'devDependencies'];
  for (const group of groups) {
    for (const name of Object.keys(pkg[group])) {
      const metadata = await download(`https://registry.npmjs.org/${encodeURIComponent(name)}/latest`);
      if (!stable.test(metadata.version)) throw new Error(`The latest tag for ${name} is not stable.`);
      if (name.startsWith('@tauri-apps/') && !metadata.version.startsWith('2.')) throw new Error('Tauri 2 is an architectural constraint. Review a newer major explicitly.');
      pkg[group][name] = metadata.version; report.npm[name] = metadata.version;
    }
  }
  if (pkg.dependencies.react !== pkg.dependencies['react-dom']) throw new Error('React and React DOM versions differ. Review upstream releases.');
  const cargoPath = join(root, 'src-tauri/Cargo.toml');
  let cargo = await readFile(cargoPath, 'utf8');
  const section = cargo.split('[workspace.dependencies]\n')[1]?.split('\n[build-dependencies]')[0];
  if (!section) throw new Error('Workspace dependency section changed; refusing an unsafe rewrite.');
  let updated = section;
  for (const line of section.split('\n')) {
    const entry = line.match(/^([a-z0-9_-]+) = (.*)$/);
    if (!entry) continue;
    const [, name, spec] = entry;
    const metadata = await download(`https://crates.io/api/v1/crates/${name}`);
    const version = metadata.crate.max_stable_version;
    if (!stable.test(version)) throw new Error(`No stable version returned for ${name}.`);
    if (name === 'tauri' && !version.startsWith('2.')) throw new Error('Tauri major changed; explicit architecture review is required.');
    if (!/"=\d+\.\d+\.\d+"/.test(spec)) throw new Error(`Unexpected dependency syntax for ${name}.`);
    updated = updated.replace(line, line.replace(/"=\d+\.\d+\.\d+"/, `"=${version}"`));
    report.crates[name] = version;
  }
  cargo = cargo.replace(section, updated);
  const rustMetadata = await download('https://static.rust-lang.org/dist/channel-rust-stable.toml', false);
  const rustVersion = rustMetadata.match(/\[pkg\.rust\]\s+version = "(\d+\.\d+\.\d+) /)?.[1];
  if (!rustVersion) throw new Error('Could not resolve the current stable Rust toolchain.');
  report.rust = rustVersion;
  const mediaMetadata = await download('https://dl.google.com/dl/android/maven2/androidx/media3/media3-exoplayer/maven-metadata.xml', false);
  const mediaVersions = [...mediaMetadata.matchAll(/<version>(\d+\.\d+\.\d+)<\/version>/g)].map((m) => m[1]);
  mediaVersions.sort((a, b) => { const aa = a.split('.').map(Number); const bb = b.split('.').map(Number); return aa[0] - bb[0] || aa[1] - bb[1] || aa[2] - bb[2]; });
  const media3 = mediaVersions.at(-1);
  if (!media3) throw new Error('Could not resolve stable Media3.');
  report.media3 = media3;
  const gradlePath = join(root, 'src-tauri/plugins/player/android/build.gradle.kts');
  const gradle = (await readFile(gradlePath, 'utf8')).replace(/val media3 = "\d+\.\d+\.\d+"/, `val media3 = "${media3}"`);
  // All metadata succeeded before any source file is changed.
  await writeAtomic(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);
  await writeAtomic(cargoPath, cargo);
  await writeAtomic(gradlePath, gradle);
  await writeAtomic(join(root, 'rust-toolchain.toml'), `[toolchain]\nchannel = "${rustVersion}"\ncomponents = ["clippy", "rustfmt"]\nprofile = "minimal"\n`);
  await writeAtomic(join(root, 'docs/dependencies-resolved.json'), `${JSON.stringify(report, null, 2)}\n`);
  run('npm', ['install']);
  run('cargo', ['generate-lockfile', '--manifest-path', 'src-tauri/Cargo.toml']);
  console.log('Exact dependency versions and lockfiles generated. Review changes, run make fmt and make check, then commit the locks.');
  console.log('A newer API may require code changes. This command never forces peer dependencies or claims build compatibility.');
}
main().catch((error) => { console.error(error instanceof Error ? error.message : 'Bootstrap failed.'); console.error('No deployment occurred. Inspect any manifest changes before retrying.'); process.exitCode = 1; });
