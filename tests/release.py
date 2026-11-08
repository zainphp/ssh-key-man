#!/usr/bin/env python3
"""Check the release workflow's publishing commands without contacting GitHub."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[1]


def publish_script():
    workflow = (ROOT / '.github/workflows/release.yml').read_text()
    step = workflow.split('      - name: Publish GitHub release\n', 1)[1]
    lines = step.split('        run: |\n', 1)[1].splitlines()
    body = []
    for line in lines:
        if line.strip() and not line.startswith('          '):
            break
        body.append(line)
    return textwrap.dedent('\n'.join(body))


class PublishingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='skm-publish-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.tools = self.root / 'tools'
        self.tools.mkdir()
        dist = self.root / 'dist'
        dist.mkdir()
        self.assets = []
        for target in ['x86_64-unknown-linux-musl', 'aarch64-unknown-linux-musl', 'x86_64-apple-darwin', 'aarch64-apple-darwin']:
            archive = f'ssh-key-man-{target}.tar.gz'
            for name in [archive, archive + '.sha256']:
                (dist / name).write_text('fixture')
                self.assets.append('dist/' + name)
        gh = self.tools / 'gh'
        gh.write_text(f'''#!{sys.executable}
import json, os, sys
arguments = sys.argv[1:]
with open(os.environ['SKM_TEST_CALLS'], 'a') as log:
    log.write(json.dumps(arguments) + '\\n')
exists = os.environ.get('SKM_TEST_EXISTS') == '1'
if arguments[:2] == ['release', 'view']:
    if exists:
        print('true' if os.environ.get('SKM_TEST_DRAFT') == '1' else 'false')
    sys.exit(0 if exists else 1)
if arguments[:2] == ['release', 'create']:
    if exists:
        print('a release with the same tag name already exists', file=sys.stderr)
        sys.exit(1)
    sys.exit(int(os.environ.get('SKM_TEST_CREATE_STATUS', '0')))
if arguments[:2] == ['release', 'upload']:
    sys.exit(int(os.environ.get('SKM_TEST_UPLOAD_STATUS', '0')))
if arguments[:2] == ['release', 'edit']:
    sys.exit(int(os.environ.get('SKM_TEST_PUBLISH_STATUS', '0')))
sys.exit(99)
''')
        gh.chmod(0o755)
        self.log = self.root / 'calls.jsonl'
        self.env = {
            **os.environ,
            'PATH': str(self.tools) + os.pathsep + os.environ['PATH'],
            'RELEASE_TAG': 'v0.1.0',
            'SKM_TEST_CALLS': str(self.log),
            'SKM_TEST_EXISTS': '0',
        }

    def publish(self, **overrides):
        self.log.unlink(missing_ok=True)
        output = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', publish_script()], cwd=self.root, env={**self.env, **overrides}, capture_output=True, text=True)
        calls = [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []
        return output, calls

    def test_new_stable_release_keeps_tag_verification_and_generated_notes(self):
        output, calls = self.publish()
        self.assertEqual(output.returncode, 0, output.stderr)
        creation = next(call for call in calls if call[:2] == ['release', 'create'])
        self.assertIn('--verify-tag', creation)
        self.assertIn('--generate-notes', creation)
        self.assertIn('--draft', creation)
        self.assertNotIn('--prerelease', creation)
        self.assertFalse(any(argument.startswith('dist/') for argument in creation))
        self.assertEqual([call[:2] for call in calls], [['release', 'view'], ['release', 'create'], ['release', 'upload'], ['release', 'edit']])
        self.assertEqual(set(self.assets), set(argument for argument in calls[-2] if argument.startswith('dist/')))
        self.assertIn('--draft=false', calls[-1])

    def test_existing_draft_uploads_assets_before_publishing_without_editing_notes(self):
        for _ in range(2):
            output, calls = self.publish(SKM_TEST_EXISTS='1', SKM_TEST_DRAFT='1')
            self.assertEqual(output.returncode, 0, output.stderr)
            self.assertEqual([call[:2] for call in calls], [['release', 'view'], ['release', 'upload'], ['release', 'edit']])
            upload = calls[-2]
            self.assertIn('--clobber', upload)
            self.assertEqual(set(self.assets), set(argument for argument in upload if argument.startswith('dist/')))
            self.assertEqual(calls[-1], ['release', 'edit', 'v0.1.0', '--draft=false'])

    def test_published_release_is_left_unchanged_on_reruns(self):
        output, calls = self.publish(SKM_TEST_EXISTS='1')
        self.assertEqual(output.returncode, 0, output.stderr)
        self.assertEqual([call[:2] for call in calls], [['release', 'view']])
        self.assertIn('already published', output.stdout)

    def test_new_prerelease_retains_its_prerelease_flag(self):
        output, calls = self.publish(RELEASE_TAG='v0.1.1-beta.1')
        self.assertEqual(output.returncode, 0, output.stderr)
        creation = next(call for call in calls if call[:2] == ['release', 'create'])
        self.assertIn('v0.1.1-beta.1', creation)
        self.assertIn('--prerelease', creation)

    def test_upload_failures_are_reported(self):
        output, calls = self.publish(SKM_TEST_EXISTS='1', SKM_TEST_DRAFT='1', SKM_TEST_UPLOAD_STATUS='23')
        self.assertEqual(output.returncode, 23)
        self.assertFalse(any(call[:2] == ['release', 'edit'] for call in calls))

    def test_creation_failures_are_reported(self):
        output, _ = self.publish(SKM_TEST_CREATE_STATUS='24')
        self.assertEqual(output.returncode, 24)

    def test_publication_failures_are_reported(self):
        output, _ = self.publish(SKM_TEST_PUBLISH_STATUS='25')
        self.assertEqual(output.returncode, 25)

    def test_missing_or_empty_assets_prevent_release_creation(self):
        for asset in self.assets:
            path = self.root / asset
            original = path.read_text()
            for missing in [False, True]:
                if missing:
                    path.unlink()
                else:
                    path.write_text('')
                output, calls = self.publish()
                self.assertNotEqual(output.returncode, 0)
                self.assertFalse(calls)
                path.write_text(original)


if __name__ == '__main__':
    unittest.main()
