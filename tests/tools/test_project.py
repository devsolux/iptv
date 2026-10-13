"""Exercise build tooling against isolated temporary repositories; never user data."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
NODE = shutil.which('node')

@unittest.skipUnless(NODE, 'Node.js is required for build-tool tests')
class ProjectTools(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='iptv-tools-')
        self.root = Path(self.temp.name) / 'project'
        shutil.copytree(ROOT, self.root, ignore=shutil.ignore_patterns(
            'node_modules', 'target', 'dist', 'gen', '__pycache__', '.git'))

    def tearDown(self):
        self.temp.cleanup()

    def run_script(self, name, *args, env=None):
        return subprocess.run([NODE, str(self.root / 'scripts' / name), *args],
                              cwd=self.root, env=env, capture_output=True, text=True, timeout=20)

    def test_cleanup_removes_only_known_outputs(self):
        removed = ['dist', 'node_modules/.cache', 'src-tauri/target',
                   'src-tauri/gen/android/app/build', 'playwright-report', 'test-results']
        retained = ['private/vault.db', 'package-lock.json', 'src-tauri/Cargo.lock',
                    'node_modules/a-package/index.js', 'src-tauri/gen/android/app/src/main/marker']
        for item in removed:
            directory = self.root / item
            directory.mkdir(parents=True)
            (directory / 'output').write_text('generated')
        for item in retained:
            path = self.root / item
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('keep')
        result = self.run_script('project.mjs', 'clean')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(all(not (self.root / item).exists() for item in removed))
        self.assertTrue(all((self.root / item).read_text() == 'keep' for item in retained))
        self.assertTrue((self.root / 'src/main.tsx').is_file())

    @unittest.skipIf(os.name == 'nt', 'Symlink test requires unprivileged symlinks')
    def test_cleanup_refuses_symlink_target(self):
        outside = Path(self.temp.name) / 'outside'
        outside.mkdir()
        (outside / 'valuable').write_text('keep')
        (self.root / 'dist').symlink_to(outside, target_is_directory=True)
        result = self.run_script('project.mjs', 'clean-web')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('symlink', result.stderr)
        self.assertEqual((outside / 'valuable').read_text(), 'keep')

    def manifest(self):
        path = self.root / 'src-tauri/gen/android/app/src/main/AndroidManifest.xml'
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('''<manifest xmlns:android="http://schemas.android.com/apk/res/android">
<uses-feature android:name="android.hardware.touchscreen" android:required="true"/>
<application android:allowBackup="true"><activity android:name=".MainActivity">
<intent-filter><category android:name="android.intent.category.LAUNCHER" /></intent-filter>
</activity></application></manifest>''')
        return path

    def test_android_configuration_is_idempotent_and_scoped(self):
        path = self.manifest()
        first = self.run_script('android-configure.mjs')
        self.assertEqual(first.returncode, 0, first.stderr)
        expected = path.read_text()
        self.assertIn('android:allowBackup="false"', expected)
        self.assertIn('LEANBACK_LAUNCHER', expected)
        self.assertNotIn('android:required="true"', expected)
        second = self.run_script('android-configure.mjs')
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(path.read_text(), expected)
        checked = self.run_script('android-configure.mjs', '--check')
        self.assertEqual(checked.returncode, 0, checked.stderr)
        release = (path.parent / 'res/xml/iptv_network_security_config.xml').read_text()
        debug = (path.parent.parent / 'debug/res/xml/iptv_network_security_config.xml').read_text()
        self.assertIn('<base-config cleartextTrafficPermitted="false"', release)
        self.assertIn('127.0.0.1', release)
        self.assertIn('<base-config cleartextTrafficPermitted="true"', debug)

    def test_android_preserves_user_managed_policy(self):
        path = self.manifest()
        original = path.read_text().replace('<application ', '<application android:networkSecurityConfig="@xml/custom" ')
        path.write_text(original)
        result = self.run_script('android-configure.mjs')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(path.read_text(), original)

    def test_release_gate_rejects_absent_lockfiles(self):
        for name in ['package-lock.json', 'src-tauri/Cargo.lock']:
            (self.root / name).unlink(missing_ok=True)
        result = self.run_script('project.mjs', 'release')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('missing', result.stderr)

    def test_lock_metadata_comparison_ignores_key_order(self):
        package_path = self.root / 'package.json'
        package = json.loads(package_path.read_text())
        for group in ['dependencies', 'devDependencies']:
            package[group] = {key: value if value != 'latest' else '1.2.3'
                              for key, value in package[group].items()}
        package_path.write_text(json.dumps(package))
        # Only a validator fixture, not a dependency graph or an installable lock.
        root_package = {'version': package['version']}
        for group in ['dependencies', 'devDependencies']:
            root_package[group] = dict(reversed(list(package[group].items())))
        (self.root / 'package-lock.json').write_text(json.dumps({'lockfileVersion': 3, 'packages': {'': root_package}}))
        (self.root / 'src-tauri/Cargo.lock').write_text('# Validator presence fixture only\n')
        result = self.run_script('project.mjs', 'locks')
        self.assertEqual(result.returncode, 0, result.stderr)

    @unittest.skipIf(os.name == 'nt', 'Uses a POSIX stub to simulate absent Cargo')
    def test_bootstrap_preflight_failure_does_not_edit_manifests(self):
        tools = Path(self.temp.name) / 'tools'
        tools.mkdir()
        npm = tools / 'npm'
        npm.write_text('#!/bin/sh\necho 10.9.2\n')
        npm.chmod(0o755)
        manifests = ['package.json', 'src-tauri/Cargo.toml', 'rust-toolchain.toml',
                     'src-tauri/plugins/player/android/build.gradle.kts']
        before = {name: hashlib.sha256((self.root / name).read_bytes()).hexdigest() for name in manifests}
        env = dict(os.environ, PATH=str(tools))
        result = self.run_script('bootstrap.mjs', env=env)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('cargo', result.stderr)
        after = {name: hashlib.sha256((self.root / name).read_bytes()).hexdigest() for name in manifests}
        self.assertEqual(before, after)

if __name__ == '__main__':
    unittest.main()
