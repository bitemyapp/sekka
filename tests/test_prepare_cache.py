"""Real Nix signing, selective NAR export and substitution across caches."""
from contextlib import contextmanager, redirect_stderr
from functools import partial
from http.server import BaseHTTPRequestHandler, SimpleHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from threading import Thread
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
sys.path.insert(0, str(SCRIPTS))
import prepare_cache as publisher

# Valid, but nobody's: a path accepted with this key alone needs no signature.
UNRELATED_KEY = 'unrelated:' + 'A' * 43 + '='


class Publication(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='sekka-publication-test-')
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.addCleanup(patch.stopall)
        patch.dict(os.environ, {
            'NIX_REMOTE': f'local?root={self.directory / "store"}',
            'NIX_CONFIG': ('experimental-features = nix-command flakes\nsubstituters =\n'
                           'sandbox = false\nbuild-users-group =\ndownload-attempts = 1\n'),
            'XDG_CACHE_HOME': str(self.directory / 'metadata'),
            'XDG_STATE_HOME': str(self.directory / 'state'),
        }).start()
        # The leaf is content-addressed, like a fetched source. The two built
        # outputs above it are not: only a signature makes a client take them.
        expression = '''let
          leaf = builtins.toFile "publication-leaf" "fixture payload";
          make = name: text: builtins.derivation {
            inherit name; system = builtins.currentSystem;
            builder = SHELL; args = [ "-c" "echo ${text} > $out" ];
          };
          middle = make "publication-middle" "${leaf}";
          root = make "publication-root" "${middle}";
        in { inherit leaf; root = root.outPath; drv = root.drvPath; middle = middle.outPath; }
        '''.replace('SHELL', self.fixture_shell())
        paths = json.loads(self.run_nix('nix', 'eval', '--impure', '--json', '--expr', expression))
        self.run_nix('nix-store', '--realise', paths['drv'])
        self.root, self.middle, self.leaf = paths['root'], paths['middle'], paths['leaf']
        self.assertEqual(self.run_nix('nix-store', '-q', '--references', self.root).split(), [self.middle])
        self.assertEqual(self.run_nix('nix-store', '-q', '--references', self.middle).split(), [self.leaf])
        self.key, self.public = self.directory / 'key', self.directory / 'public'
        self.run_nix('nix-store', '--generate-binary-cache-key', 'publication-test', str(self.key), str(self.public))
        self.upstream, self.destination = self.directory / 'upstream', self.directory / 'destination'
        self.run_nix('nix', 'copy', '--to', self.upstream.as_uri(), self.leaf)
        self.destination.mkdir()
        (self.destination / 'nix-cache-info').write_text('StoreDir: /nix/store\n')

    def fixture_shell(self):
        # Import the packaged check's static shell into each isolated store.
        # The nested builder needs no host-store registration. Recipes echo.
        shell_expression = json.dumps('/bin/sh')
        if os.environ.get('TEST_SHELL'):
            directory = self.directory / 'builder'
            directory.mkdir()
            shell = directory / 'sh'
            shutil.copyfile(os.environ['TEST_SHELL'], shell)
            shell.chmod(0o755)
            shell_expression = ('(builtins.path { path = ' + json.dumps(str(directory))
                                + '; name = "fixture-shell"; }) + "/sh"')
        return shell_expression

    @staticmethod
    def run_nix(*args, **kwargs):
        result = subprocess.run(args, text=True, capture_output=True, timeout=60, **kwargs)
        if result.returncode:
            raise AssertionError(result.stderr)
        return result.stdout

    @staticmethod
    def narinfo(directory, path):
        return directory / (Path(path).name.split('-', 1)[0] + '.narinfo')

    def archive(self, directory, path):
        fields = dict(line.split(': ', 1) for line in self.narinfo(directory, path).read_text().splitlines()
                      if ': ' in line)
        return directory / fields['URL']

    def stage(self, name='stage', destination=None):
        directory = self.directory / name
        paths = publisher.prepare([self.root], directory, self.key,
                                  [self.upstream.as_uri()], destination or self.destination.as_uri())
        return directory, paths

    def publish(self):
        """A cold publication, then its result as the published cache."""
        stage, _ = self.stage()
        shutil.copytree(stage, self.destination, dirs_exist_ok=True)

    @contextmanager
    def http_cache(self, directory):
        requests = []
        class Handler(SimpleHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_HEAD(self):
                requests.append(('HEAD', self.path))
                super().do_HEAD()

            def do_GET(self):
                requests.append(('GET', self.path))
                super().do_GET()

        server = ThreadingHTTPServer(('127.0.0.1', 0), partial(Handler, directory=str(directory)))
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield f'http://127.0.0.1:{server.server_port}', requests
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_only_missing_paths_are_exported_signed_and_substitutable(self):
        stage, paths = self.stage()
        self.assertEqual(set(paths), {self.root, self.middle})
        self.assertEqual(len(list((stage / 'nar').iterdir())), 2)
        self.assertFalse(self.narinfo(stage, self.leaf).exists())
        for path in paths:
            self.assertIn('Sig: publication-test:', self.narinfo(stage, path).read_text())
        public = self.public.read_text().strip()
        self.run_nix('nix', 'store', 'verify', '--store', stage.as_uri(), '--no-contents',
                     '--sigs-needed', '1', '--trusted-public-keys', public, *paths)
        # The signed stage intentionally lacks its upstream reference. A
        # fresh client must fetch from both caches, with signature enforcement.
        client = self.directory / 'client'
        self.run_nix('nix-store', '--store', f'local?root={client}', '--realise', self.root,
                     '--option', 'substituters', f'{stage.as_uri()} {self.upstream.as_uri()}',
                     '--option', 'trusted-public-keys', public, '--option', 'require-sigs', 'true')
        for path in (self.root, self.middle, self.leaf):
            self.assertTrue((client / path.lstrip('/')).exists())

    def test_content_addressed_paths_keep_their_ca_field(self):
        # Without its upstream, the fetched-source-like leaf is published too,
        # and a client may accept it by its contents, as after nix copy.
        self.narinfo(self.upstream, self.leaf).unlink()
        stage, paths = self.stage()
        self.assertEqual(set(paths), {self.root, self.middle, self.leaf})
        self.assertIn('\nCA: text:sha256:', self.narinfo(stage, self.leaf).read_text())
        self.assertNotIn('CA:', self.narinfo(stage, self.root).read_text())
        self.run_nix('nix', 'store', 'verify', '--store', stage.as_uri(), '--no-contents',
                     '--sigs-needed', '1', '--trusted-public-keys', UNRELATED_KEY, self.leaf)

    def test_warm_publication_creates_no_archives(self):
        self.publish()
        with patch.object(publisher, 'export_path', side_effect=AssertionError('warm path was exported')):
            warm, paths = self.stage('warm')
        self.assertEqual(paths, [])
        self.assertEqual(list((warm / 'nar').iterdir()), [])
        self.assertEqual(list(warm.glob('*.narinfo')), [])

    def test_one_new_output_does_not_export_unchanged_references(self):
        self.publish()
        self.root = self.run_nix('nix', 'eval', '--raw', '--impure', '--expr',
                                 f'let ref = builtins.storePath "{self.middle}"; '
                                 'in builtins.toFile "publication-edited" ("changed: " + ref)').strip()
        self.assertEqual(self.run_nix('nix-store', '-q', '--references', self.root).strip(), self.middle)
        edited, paths = self.stage('edited')
        self.assertEqual(paths, [self.root])
        self.assertEqual(len(list((edited / 'nar').iterdir())), 1)

    def test_missing_and_truncated_existing_archives_are_repaired(self):
        self.publish()
        self.archive(self.destination, self.root).unlink()
        self.archive(self.destination, self.middle).write_bytes(b'truncated')
        repaired, paths = self.stage('repaired')
        self.assertEqual(set(paths), {self.root, self.middle})
        self.assertEqual(len(list((repaired / 'nar').iterdir())), 2)

    def test_published_path_built_differently_is_kept(self):
        self.publish()
        narinfo = self.narinfo(self.destination, self.root)
        narinfo.write_text(narinfo.read_text().replace('NarSize: ', 'NarSize: 1'))
        with redirect_stderr(io.StringIO()) as messages:
            kept, paths = self.stage('kept')
        self.assertEqual(paths, [])
        self.assertEqual(list((kept / 'nar').iterdir()), [])
        self.assertIn(f'Keeping the published copy of {self.root}', messages.getvalue())

    def test_http_destination_is_probed_without_downloading_archives(self):
        self.publish()
        self.archive(self.destination, self.middle).unlink()
        with self.http_cache(self.destination) as (url, requests):
            served, paths = self.stage('served', destination=url)
        self.assertEqual(paths, [self.middle])
        self.assertEqual(len(list((served / 'nar').iterdir())), 1)
        archives = [(method, path) for method, path in requests if path.startswith('/nar/')]
        self.assertEqual(len(archives), 2)
        self.assertTrue(all(method == 'HEAD' for method, _ in archives), archives)

    def test_cache_errors_fail_before_any_export(self):
        class BrokenCache(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_error(500)

            def log_message(self, *_):
                pass
        server = ThreadingHTTPServer(('127.0.0.1', 0), BrokenCache)
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with patch.object(publisher, 'export_path', side_effect=AssertionError('export after query error')):
                with self.assertRaises(subprocess.CalledProcessError):
                    publisher.prepare([self.root], self.directory / 'failed', self.key,
                                      [f'http://127.0.0.1:{server.server_port}'], self.destination.as_uri())
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_bad_key_cannot_produce_a_successful_stage(self):
        self.key.write_text('invalid key')
        with self.assertRaises(subprocess.CalledProcessError):
            self.stage()

    def test_signatures_are_rejected_with_an_untrusted_public_key(self):
        stage, paths = self.stage()
        result = subprocess.run(['nix', 'store', 'verify', '--store', stage.as_uri(), '--no-contents',
                                 '--sigs-needed', '1', '--trusted-public-keys', UNRELATED_KEY,
                                 *paths], text=True, capture_output=True, timeout=30)
        self.assertNotEqual(result.returncode, 0)

    def test_inconsistent_local_nar_metadata_is_rejected(self):
        stage = self.directory / 'corrupt'
        (stage / 'nar').mkdir(parents=True)
        info = json.loads(self.run_nix('nix', 'path-info', '--json', '--json-format', '1', self.root))[self.root]
        info['narSize'] += 1
        with self.assertRaisesRegex(ValueError, 'does not match registered contents'):
            publisher.export_path(self.root, info, stage)
        self.assertEqual(list(stage.glob('*.narinfo')), [])
        self.assertEqual(list((stage / 'nar').iterdir()), [])

    def test_failed_export_reports_the_exporter_message(self):
        stage = self.directory / 'absent'
        (stage / 'nar').mkdir(parents=True)
        info = json.loads(self.run_nix('nix', 'path-info', '--json', '--json-format', '1', self.root))[self.root]
        missing = '/nix/store/' + '0' * 32 + '-publication-missing'
        with self.assertRaises(ValueError) as failure:
            publisher.export_path(missing, info, stage)
        self.assertIn(f'NAR export failed for {missing}: ', str(failure.exception))
        self.assertIn(f"path '{missing}' is not a valid store path", str(failure.exception))
        self.assertEqual(list(stage.iterdir()), [stage / 'nar'])

    def test_preserves_determinate_nixd_exclusion(self):
        path = '/nix/store/' + '0' * 32 + '-determinate-nixd-fixture'
        self.assertEqual(publisher.unpublished({path: {}}, [], self.destination.as_uri()), [])


if __name__ == '__main__':
    unittest.main()
