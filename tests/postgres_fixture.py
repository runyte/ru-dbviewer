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
