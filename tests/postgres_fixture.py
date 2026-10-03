# SPDX-License-Identifier: MPL-2.0
"""Check native PostgreSQL client isolation without a running database server."""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('postgres_tests', ROOT / 'scripts/postgres_tests.py')
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class PostgresFixtureTests(unittest.TestCase):
    def test_cluster_commands_isolate_openssl_config_and_preserve_coverage(self):
        stop_error = subprocess.CalledProcessError(1, ['fixture-pg_ctl', 'stop'])
        stopped = subprocess.CalledProcessError(3, ['fixture-pg_ctl', 'status'])
        with mock.patch.dict(os.environ, {
            'OPENSSL_CONF': '/fixture/caller/openssl.cnf',
            'OPENSSL_CONF_INCLUDE': '/fixture/caller/includes',
            'LLVM_PROFILE_FILE': '/fixture/coverage/%p.profraw',
        }):
            with mock.patch.object(fixture, 'run', side_effect=[None, stop_error, stopped]) as run:
                with fixture.NativeCluster() as cluster:
                    cluster.start()
            self.assertEqual(os.environ['OPENSSL_CONF'], '/fixture/caller/openssl.cnf')
        self.assertEqual([call.args[0][-1] for call in run.call_args_list],
                         ['start', 'stop', 'status'])
        for call in run.call_args_list:
            env = call.kwargs['env']
            self.assertEqual(env['OPENSSL_CONF'], str(cluster.root / 'openssl.cnf'))
            self.assertEqual(env['OPENSSL_CONF_INCLUDE'], str(cluster.root))
            self.assertEqual(env['LLVM_PROFILE_FILE'], '/fixture/coverage/%p.profraw')

    def test_successful_cluster_stops_before_removing_storage(self):
        with mock.patch.object(fixture, 'run') as run:
            with fixture.NativeCluster() as cluster:
                cluster.start()
                self.assertTrue(cluster.root.is_dir())
            self.assertFalse(cluster.root.exists())
        self.assertEqual([call.args[0][-1] for call in run.call_args_list], ['start', 'stop'])
        for call in run.call_args_list:
            args = call.args[0]
            self.assertEqual(args[args.index('-D') + 1], cluster.root / 'data')
            self.assertEqual(args[args.index('-t') + 1], '60')

    def test_failed_start_still_stops_and_preserves_original_error(self):
        original = subprocess.CalledProcessError(1, ['fixture-pg_ctl', 'start'])
        with mock.patch.object(fixture, 'run', side_effect=[original, None]) as run:
            with self.assertRaises(subprocess.CalledProcessError) as error:
                with fixture.NativeCluster() as cluster:
                    cluster.start()
            self.assertIs(error.exception, original)
            self.assertFalse(cluster.root.exists())
        self.assertEqual([call.args[0][-1] for call in run.call_args_list], ['start', 'stop'])

    def test_failed_start_accepts_only_confirmed_no_server_status(self):
        original = subprocess.CalledProcessError(1, ['fixture-pg_ctl', 'start'])
        stop_error = subprocess.CalledProcessError(1, ['fixture-pg_ctl', 'stop'])
        stopped = subprocess.CalledProcessError(3, ['fixture-pg_ctl', 'status'])
        with mock.patch.object(fixture, 'run', side_effect=[original, stop_error, stopped]) as run:
            with self.assertRaises(subprocess.CalledProcessError) as error:
                with fixture.NativeCluster() as cluster:
                    cluster.start()
            self.assertIs(error.exception, original)
            self.assertFalse(cluster.root.exists())
        self.assertEqual([call.args[0][-1] for call in run.call_args_list],
                         ['start', 'stop', 'status'])

    def test_running_server_after_failed_stop_retains_storage_and_start_error(self):
        original = subprocess.CalledProcessError(1, ['fixture-pg_ctl', 'start'])
        stop_error = subprocess.CalledProcessError(1, ['fixture-pg_ctl', 'stop'])
        with mock.patch.object(fixture, 'run', side_effect=[original, stop_error, None]):
            with self.assertRaises(subprocess.CalledProcessError) as error:
                with fixture.NativeCluster() as cluster:
                    cluster.start()
        try:
            self.assertIs(error.exception, original)
            self.assertTrue(cluster.root.is_dir())
            self.assertIn(str(cluster.root), '\n'.join(original.__notes__))
        finally:
            shutil.rmtree(cluster.root)

    def test_unknown_status_after_failed_stop_retains_storage_and_reports_failure(self):
        stop_error = subprocess.CalledProcessError(1, ['fixture-pg_ctl', 'stop'])
        unknown = subprocess.CalledProcessError(4, ['fixture-pg_ctl', 'status'])
        with mock.patch.object(fixture, 'run', side_effect=[None, stop_error, unknown]):
            with self.assertRaises(RuntimeError) as error:
                with fixture.NativeCluster() as cluster:
                    cluster.start()
        try:
            self.assertTrue(cluster.root.is_dir())
            self.assertIn(str(cluster.root), str(error.exception))
        finally:
            shutil.rmtree(cluster.root)

    def test_failure_before_start_removes_storage_without_pg_ctl(self):
        original = RuntimeError('fixture initialization failed')
        with mock.patch.object(fixture, 'run') as run:
            with self.assertRaises(RuntimeError) as error:
                with fixture.NativeCluster() as cluster:
                    raise original
            self.assertIs(error.exception, original)
            self.assertFalse(cluster.root.exists())
            run.assert_not_called()

    @unittest.skipUnless(shutil.which('openssl'), 'installed openssl is unavailable')
    def test_certificates_ignore_caller_config_and_keep_tls_extensions(self):
        with tempfile.TemporaryDirectory(prefix='dbv-certificates-') as directory:
            root = Path(directory)
            caller_config = root / 'caller.cnf'
            caller_config.write_text('fixture-invalid-openssl-config\n')
            with mock.patch.dict(os.environ, {'OPENSSL_CONF': str(caller_config)}):
                fixture.create_certificates(root)
                self.assertEqual(os.environ['OPENSSL_CONF'], str(caller_config))
            env = {**os.environ, 'OPENSSL_CONF': str(root / 'openssl.cnf'),
                   'OPENSSL_CONF_INCLUDE': str(root)}
            for name, purpose, hostname in [('server', 'sslserver', 'localhost'),
                                             ('client', 'sslclient', 'dbviewer_cert')]:
                result = fixture.run([
                    'openssl', 'verify', '-CAfile', root / 'ca.crt',
                    '-purpose', purpose, '-verify_hostname', hostname, root / f'{name}.crt',
                ], env=env)
                self.assertIn(': OK', result.stdout)
                self.assertEqual((root / f'{name}.key').stat().st_mode & 0o777, 0o600)
            with self.assertRaises(subprocess.CalledProcessError):
                fixture.run(['openssl', 'verify', '-CAfile', root / 'ca.crt',
                             '-purpose', 'sslserver', root / 'client.crt'], env=env)
            with self.assertRaises(subprocess.CalledProcessError):
                fixture.run(['openssl', 'verify', '-CAfile', root / 'ca.crt',
                             '-verify_hostname', 'wrong.fixture', root / 'server.crt'], env=env)

    def test_client_environment_isolates_options_and_preserves_coverage(self):
        with tempfile.TemporaryDirectory(prefix='dbv-client-env-') as directory:
            root = Path(directory)
            inherited = {
                'PATH': '/fixture/tools', 'HOME': '/fixture/caller',
                'PGHOSTADDR': 'fixture-address', 'PGSERVICE': 'fixture-service',
                'PGSERVICEFILE': '/fixture/caller/service',
                'PGPASSFILE': '/fixture/caller/passfile', 'PGOPTIONS': '-c fixture=on',
                'PGFUTUREOPTION': 'must-be-removed',
                'LLVM_PROFILE_FILE': '/fixture/coverage/%p.profraw',
                'CARGO_BUILD_JOBS': '1',
            }
            with mock.patch.dict(os.environ, inherited, clear=True):
                with mock.patch.object(fixture, 'run') as run:
                    fixture.create_database(root, 65432)
                self.assertEqual(dict(os.environ), inherited)
            args = run.call_args.args[0]
            env = run.call_args.kwargs['env']
            for key in ['PGHOSTADDR', 'PGSERVICE', 'PGOPTIONS', 'PGFUTUREOPTION']:
                self.assertNotIn(key, env)
            self.assertEqual(env['HOME'], str(root))
            self.assertEqual(env['PGSERVICEFILE'], str(root / 'pg_service.conf'))
            self.assertEqual(env['PGSYSCONFDIR'], str(root))
            self.assertEqual(env['OPENSSL_CONF'], str(root / 'openssl.cnf'))
            self.assertEqual(env['OPENSSL_CONF_INCLUDE'], str(root))
            self.assertEqual(Path(env['PGPASSFILE']).read_text(), '')
            self.assertEqual(Path(env['PGPASSFILE']).stat().st_mode & 0o777, 0o600)
            for key in ['PATH', 'LLVM_PROFILE_FILE', 'CARGO_BUILD_JOBS']:
                self.assertEqual(env[key], inherited[key])
            self.assertIn('-X', args)
            self.assertIn('-w', args)

    def test_installed_client_ignores_caller_service_and_hostaddr(self):
        if not shutil.which(fixture.pg('psql')):
            self.skipTest('installed psql is unavailable')
        with tempfile.TemporaryDirectory(prefix='dbv-client-connection-') as directory:
            root = Path(directory)
            caller = root / 'caller'
            caller.mkdir()
            (caller / 'service.conf').write_text(
                '[fixture-service]\nhostaddr=invalid-fixture-hostaddr\n')
            # Even explicitly setting -h does not override libpq's hostaddr.
            # The real helper must ignore these settings before invoking psql.
            with mock.patch.dict(os.environ, {
                'PGHOSTADDR': 'invalid-fixture-hostaddr',
                'PGSERVICE': 'fixture-service',
                'PGSERVICEFILE': str(caller / 'service.conf'),
                'PGPASSFILE': str(caller / 'passfile'),
            }):
                with self.assertRaises(subprocess.CalledProcessError) as error:
                    fixture.create_database(root, 65432)
            stderr = error.exception.stderr
            self.assertIn(str(root / '.s.PGSQL.65432'), stderr)
            self.assertNotIn('invalid-fixture-hostaddr', stderr)
            self.assertNotIn('fixture-service', stderr)


if __name__ == '__main__':
    unittest.main()
