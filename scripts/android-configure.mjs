import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { root } from './project.mjs';

const base = join(root, 'src-tauri/gen/android/app/src');
const manifestPath = join(base, 'main/AndroidManifest.xml');
const marker = 'IPTV generated network policy';
const releasePolicy = `<?xml version="1.0" encoding="utf-8"?>
<!-- ${marker}. Upstream HTTP is authorized in Rust; Java only uses loopback. -->
<network-security-config>
  <base-config cleartextTrafficPermitted="false" />
  <domain-config cleartextTrafficPermitted="true"><domain includeSubdomains="false">127.0.0.1</domain></domain-config>
</network-security-config>\n`;
const debugPolicy = `<?xml version="1.0" encoding="utf-8"?>
<!-- ${marker}. DEBUG ONLY: permits the Vite development host. -->
<network-security-config><base-config cleartextTrafficPermitted="true" /></network-security-config>\n`;
function put(path, content) {
  if (existsSync(path) && !readFileSync(path, 'utf8').includes(marker))
    throw new Error(`Refusing to replace a user-managed network policy: ${path}`);
  writeFileSync(path, content);
}
try {
  if (!existsSync(manifestPath))
    throw new Error('Run npm run android:init first; the Tauri-generated Android host is missing.');
  let manifest = readFileSync(manifestPath, 'utf8');
  const check = process.argv.includes('--check');
  if (check) {
    if (
      !manifest.includes('android.intent.category.LEANBACK_LAUNCHER') ||
      !manifest.includes('android:networkSecurityConfig="@xml/iptv_network_security_config"') ||
      !manifest.includes('android:banner="@drawable/iptv_banner"')
    )
      throw new Error(
        'Android TV/network configuration is missing. Run node scripts/android-configure.mjs.',
      );
    if (
      readFileSync(join(base, 'main/res/xml/iptv_network_security_config.xml'), 'utf8') !==
      releasePolicy
    )
      throw new Error('Release network policy differs from the reviewed scoped loopback policy.');
    console.log(
      'Android TV manifest and release loopback policy: OK. Verify the merged release manifest on each toolchain.',
    );
  } else {
    if (
      !manifest.includes('<application') ||
      !manifest.includes('android.intent.category.LAUNCHER')
    )
      throw new Error('Unrecognized generated manifest; refusing to guess its structure.');
    const existing = manifest.match(/android:networkSecurityConfig="([^"]+)"/);
    if (existing && existing[1] !== '@xml/iptv_network_security_config')
      throw new Error('An existing custom network security policy needs a manual review.');
    if (!existing)
      manifest = manifest.replace(
        '<application',
        '<application android:networkSecurityConfig="@xml/iptv_network_security_config"',
      );
    if (/android:allowBackup=/.test(manifest))
      manifest = manifest.replace(/android:allowBackup="[^"]*"/, 'android:allowBackup="false"');
    else manifest = manifest.replace('<application', '<application android:allowBackup="false"');
    if (!manifest.includes('android:banner='))
      manifest = manifest.replace(
        '<application',
        '<application android:banner="@drawable/iptv_banner"',
      );
    if (!manifest.includes('android.intent.category.LEANBACK_LAUNCHER'))
      manifest = manifest.replace(
        /(<category\s+android:name="android.intent.category.LAUNCHER"\s*\/>)/,
        '$1\n                <category android:name="android.intent.category.LEANBACK_LAUNCHER" />',
      );
    for (const feature of ['android.hardware.touchscreen', 'android.software.leanback']) {
      if (!manifest.includes(`android:name="${feature}"`))
        manifest = manifest.replace(
          '<application',
          `<uses-feature android:name="${feature}" android:required="false" />\n    <application`,
        );
      else
        manifest = manifest.replace(
          new RegExp(
            `<uses-feature[^>]*android:name="${feature.replaceAll('.', '\\.')}"[^>]*/>`,
            'g',
          ),
          `<uses-feature android:name="${feature}" android:required="false" />`,
        );
    }
    for (const path of ['main/res/xml', 'debug/res/xml', 'main/res/drawable'])
      mkdirSync(join(base, path), { recursive: true });
    put(join(base, 'main/res/xml/iptv_network_security_config.xml'), releasePolicy);
    put(join(base, 'debug/res/xml/iptv_network_security_config.xml'), debugPolicy);
    copyFileSync(
      join(root, 'src-tauri/icons/tv-banner.png'),
      join(base, 'main/res/drawable/iptv_banner.png'),
    );
    writeFileSync(manifestPath, manifest);
    console.log(
      'Configured optional TV features, launcher banner, scoped release networking and debug-only Vite networking.',
    );
    console.log(
      'Media3 requires a compatible Kotlin/AGP/JDK/SDK toolchain. Review generated Gradle files before building.',
    );
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : 'Android configuration failed.');
  process.exitCode = 1;
}
