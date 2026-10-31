"""Runtime coverage against real Nix metadata and disposable file/HTTP caches."""
from contextlib import contextmanager, redirect_stdout
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from threading import Thread
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/check_cache_coverage.py'
SPEC = importlib.util.spec_from_file_location('coverage', SCRIPT)
coverage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(coverage)


class CacheCoverage(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='sekka-coverage-test-')
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.addCleanup(patch.stopall)
        patch.dict(os.environ, {
            'NIX_REMOTE': f'local?root={self.directory / "store"}',
            'NIX_CONFIG': ('experimental-features = nix-command flakes\nsubstituters =\n'
                           'sandbox = false\nbuild-users-group =\n'),
            'XDG_CACHE_HOME': str(self.directory / 'metadata'),
            'XDG_STATE_HOME': str(self.directory / 'state'),
        }).start()
        expression = '''let
          leaf = builtins.toFile "coverage-leaf" "fixture payload";
          middle = builtins.toFile "coverage-middle" "${leaf}";
        in builtins.toFile "coverage-root" "${middle}"'''
        self.root = self.command('nix', 'eval', '--raw', '--expr', expression).strip()
        self.middle = self.command('nix-store', '-q', '--references', self.root).strip()
        self.leaf = self.command('nix-store', '-q', '--references', self.middle).strip()
        self.first, self.second = self.directory / 'first', self.directory / 'second'
        self.copy(self.first, self.root)

    @staticmethod
    def command(*args, input=None):
        result = subprocess.run(args, input=input, text=True, capture_output=True, timeout=30)
        if result.returncode:
            raise AssertionError(f'{args[0]} failed: {result.stderr}')
        return result.stdout

    def copy(self, directory, path):
        self.command('nix', 'copy', '--to', f'file://{directory}?compression=none', path)

    @staticmethod
    def narinfo(directory, path):
        return directory / (Path(path).name.split('-', 1)[0] + '.narinfo')

    def archive(self, directory, path):
        fields = dict(line.split(': ', 1) for line in self.narinfo(directory, path).read_text().splitlines()
                      if ': ' in line)
        return directory / fields['URL']

    def checker(self, *caches):
        return coverage.CacheCoverage(list(caches) or [self.first.as_uri()])

    def verify(self, checker):
        # This is the old gate's false-positive condition: the root exists.
        self.assertEqual(checker.uncached([self.root]), set())
        output = io.StringIO()
        with redirect_stdout(output):
            coverage.check_runtime(checker)
        self.assertIn('runtime: 3 available archives; 0 build fallbacks', output.getvalue())

    @contextmanager
    def http_cache(self, no_head=False):
        requests = []
        class Handler(SimpleHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_HEAD(self):
                requests.append(('HEAD', self.path, None))
                if no_head and self.path.startswith('/nar/'):
                    self.send_error(405)
                else:
                    super().do_HEAD()

            def do_GET(self):
                requests.append(('GET', self.path, self.headers.get('Range')))
                super().do_GET()

        server = ThreadingHTTPServer(('127.0.0.1', 0), partial(Handler, directory=str(self.first)))
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield f'http://127.0.0.1:{server.server_port}', requests
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_cached_root_walks_transitive_references(self):
        self.verify(self.checker())

    def test_cached_root_with_missing_transitive_reference_fails(self):
        self.narinfo(self.first, self.leaf).unlink()
        checker = self.checker()
        self.assertEqual(checker.uncached([self.root]), set())
        with self.assertRaisesRegex(ValueError, f'{self.leaf}.*required by {self.middle}'):
            coverage.check_runtime(checker)

    def test_runtime_closure_can_span_caches(self):
        self.copy(self.second, self.leaf)
        self.narinfo(self.first, self.leaf).unlink()
        self.verify(self.checker(self.first.as_uri(), self.second.as_uri()))

    def test_http_cache_checks_archives_without_downloading_them(self):
        with self.http_cache() as (url, requests):
            self.verify(self.checker(url))
        archives = [(method, path) for method, path, _ in requests if path.startswith('/nar/')]
        self.assertEqual(len(archives), 3)
        self.assertTrue(all(method == 'HEAD' for method, _ in archives), archives)

    def test_missing_http_archive_fails_despite_present_metadata(self):
        self.archive(self.first, self.leaf).unlink()
        with self.http_cache() as (url, _):
            checker = self.checker(url)
            self.assertEqual(checker.uncached([self.root]), set())
            with self.assertRaisesRegex(ValueError, 'HTTP Error 404'):
                coverage.check_runtime(checker)

    def test_head_unsupported_uses_bounded_range_request(self):
        with self.http_cache(no_head=True) as (url, requests):
            self.verify(self.checker(url))
        gets = [range_header for method, path, range_header in requests
                if method == 'GET' and path.startswith('/nar/')]
        self.assertEqual(gets, ['bytes=0-0'] * 3)

    def test_missing_archive_can_be_obtained_from_another_cache(self):
        self.copy(self.second, self.leaf)
        self.archive(self.first, self.leaf).unlink()
        self.verify(self.checker(self.first.as_uri(), self.second.as_uri()))

    def test_truncated_archive_fails(self):
        self.archive(self.first, self.leaf).write_bytes(b'truncated')
        with self.assertRaisesRegex(ValueError, 'archive length 9, expected'):
            self.verify(self.checker())

    def test_main_rejects_incomplete_runtime_of_an_otherwise_cached_root(self):
        self.narinfo(self.first, self.leaf).unlink()
        drv = '/nix/store/' + '0' * 32 + '-fixture.drv'
        info = {'name': 'fixture', 'env': {}, 'outputs': {'out': {'path': self.root}},
                'inputs': {'drvs': {}}}
        with patch.object(coverage, 'nix', return_value=drv), \
                patch.object(coverage, 'show', return_value={drv: info}), \
                redirect_stdout(io.StringIO()) as stdout:
            with self.assertRaisesRegex(ValueError, 'Incomplete runtime cache coverage'):
                coverage.main(['--cache', self.first.as_uri(), 'fixture'])

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

    def fallback_fixture(self, parent_local=True, transitive_compile=False):
        expression = '''let
          make = name: text: extra: builtins.derivation ({
            inherit name; system = builtins.currentSystem;
            builder = SHELL; args = [ "-c" "echo ${text} > $out" ];
          } // extra);
          stdenv = make "stdenv-linux-no-cc" "environment" { preferLocalBuild = true; };
          compiled = make "coverage-compiled" "compiled" {};
          assembly = make "coverage-assembly" "${stdenv} EXTRA" {};
          root = make "coverage-parent" "${assembly}" { preferLocalBuild = LOCAL; };
        in { root = root.outPath; drv = root.drvPath; assembly = assembly.outPath;
             assemblyDrv = assembly.drvPath; stdenv = stdenv.outPath; compiled = compiled.outPath; }
        '''.replace('SHELL', self.fixture_shell()).replace('LOCAL', str(parent_local).lower())
        expression = expression.replace('EXTRA', '${compiled}' if transitive_compile else '')
        paths = json.loads(self.command('nix', 'eval', '--impure', '--json', '--expr', expression))
        self.command('nix-store', '--realise', paths['drv'])
        key = self.directory / 'key.sec'
        key.write_text(self.command('nix', 'key', 'generate-secret', '--key-name', 'coverage-test'))
        key.chmod(0o600)
        public_key = self.command('nix', 'key', 'convert-secret-to-public', input=key.read_text()).strip()
        self.command('nix', 'copy', '--to', f'file://{self.first}?secret-key={key}', paths['root'])
        self.narinfo(self.first, paths['assembly']).unlink()
        if transitive_compile:
            self.narinfo(self.first, paths['compiled']).unlink()
        return paths, public_key

    def test_native_nix_rebuilds_missing_assembly_and_cached_parent(self):
        paths, public_key = self.fallback_fixture()
        # Copy only recipes into a fresh store; the original outputs cannot
        # satisfy the client. Signed cache data and normal Nix trust apply.
        client = f'local?root={self.directory / "client"}'
        self.command('nix', 'copy', '--derivation', '--to', client, paths['drv'])
        result = subprocess.run([
            'nix-store', '--store', client, '--realise', paths['drv'],
            '--option', 'substituters', self.first.as_uri(),
            '--option', 'trusted-public-keys', public_key,
        ], capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"building '{paths['assemblyDrv']}'", result.stderr)
        self.assertIn(f"building '{paths['drv']}'", result.stderr)
        self.assertTrue((self.directory / 'client' / paths['root'].lstrip('/')).is_file())
        with redirect_stdout(io.StringIO()) as stdout:
            self.assertEqual(coverage.main(['--cache', self.first.as_uri(), paths['drv']]), 0)
        self.assertIn('local: 1', stdout.getvalue())
        self.assertIn('copies: 1', stdout.getvalue())
        self.assertIn('2 build fallbacks', stdout.getvalue())

    def test_cached_compiler_ancestor_is_rejected_when_runtime_input_is_missing(self):
        paths, _ = self.fallback_fixture(parent_local=False)
        with redirect_stdout(io.StringIO()) as stdout:
            self.assertEqual(coverage.main(['--cache', self.first.as_uri(), paths['drv']]), 1)
        self.assertIn('compiles: 1', stdout.getvalue())
        self.assertIn('coverage-parent', stdout.getvalue())

    def test_copy_fallback_with_transitive_uncached_compile_is_rejected(self):
        paths, _ = self.fallback_fixture(transitive_compile=True)
        with redirect_stdout(io.StringIO()) as stdout:
            self.assertEqual(coverage.main(['--cache', self.first.as_uri(), paths['drv']]), 1)
        self.assertIn('compiles: 1', stdout.getvalue())
        self.assertIn('coverage-compiled', stdout.getvalue())

    def test_cached_fallback_build_input_with_missing_archive_is_rejected(self):
        paths, _ = self.fallback_fixture()
        self.archive(self.first, paths['stdenv']).unlink()
        with redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(ValueError, 'Incomplete runtime cache coverage'):
                coverage.main(['--cache', self.first.as_uri(), paths['drv']])

    def test_later_requested_output_of_seen_derivation_cannot_hide_compile(self):
        expression = '''let
          make = name: command: extra: builtins.derivation ({
            inherit name; system = builtins.currentSystem;
            builder = SHELL; args = [ "-c" command ];
          } // extra);
          multi = make "coverage-multiple" "echo out > $out; echo dev > $dev" {
            outputs = [ "out" "dev" ];
          };
          late = make "coverage-late" "echo ${multi.dev} > $out" { preferLocalBuild = true; };
          root = make "coverage-root" "echo ${multi.out} ${late} > $out" { preferLocalBuild = true; };
        in { drv = root.drvPath; cached = multi.outPath; }
        '''.replace('SHELL', self.fixture_shell())
        paths = json.loads(self.command('nix', 'eval', '--impure', '--json', '--expr', expression))
        self.command('nix-store', '--realise', paths['drv'])
        self.copy(self.first, paths['cached'])
        # out is seen first and cached; only the uncached late input requests
        # dev. The runtime graph contains neither late nor dev, so it cannot
        # rescue a derivation traversal that skips already-seen recipes.
        with redirect_stdout(io.StringIO()) as stdout:
            self.assertEqual(coverage.main(['--cache', self.first.as_uri(), paths['drv']]), 1)
        self.assertIn('compiles: 1', stdout.getvalue())
        self.assertIn('coverage-multiple', stdout.getvalue())

    def test_malformed_metadata_is_not_treated_as_a_cache_miss(self):
        self.narinfo(self.first, self.leaf).write_text('not narinfo\n')
        with self.assertRaises(subprocess.CalledProcessError):
            self.verify(self.checker())

    def test_main_preserves_local_download_copy_and_compile_classification(self):
        # Only derivation evaluation is stubbed: cache lookups still use the
        # disposable Nix store, never the host's existing installed closure.
        drv = '/nix/store/' + '0' * 32 + '-fixture.drv'
        output = '/nix/store/' + '0' * 32 + '-fixture'
        cases = [
            ({'preferLocalBuild': '1'}, {}, {}, 'local', 0),
            ({'allowSubstitutes': ''}, {}, {}, 'local', 0),
            ({}, {'hash': 'sha256:fixture'}, {}, 'downloads', 0),
            ({}, {}, {'/nix/store/' + '0' * 32 + '-stdenv-linux-no-cc.drv': []}, 'copies', 0),
            ({}, {}, {}, 'compiles', 1),
        ]
        for env, extra, inputs, kind, expected in cases:
            with self.subTest(kind=kind, env=env):
                info = {'name': 'fixture', 'env': env, 'outputs': {'out': {'path': output, **extra}},
                        'inputs': {'drvs': inputs}}
                with patch.object(coverage, 'nix', return_value=drv), \
                        patch.object(coverage, 'show', side_effect=lambda paths: {drv: info} if drv in paths else {}), \
                        redirect_stdout(io.StringIO()) as stdout:
                    status = coverage.main(['--cache', self.first.as_uri(), 'fixture'])
                    self.assertEqual(status, expected)
                    self.assertIn(f'{kind}: 1', stdout.getvalue())
                    if kind == 'compiles':
                        self.assertEqual(coverage.main([
                            '--cache', self.first.as_uri(), '--allow', 'fixture', 'fixture']), 0)


if __name__ == '__main__':
    unittest.main()
