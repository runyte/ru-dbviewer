# SPDX-License-Identifier: MPL-2.0
"""Public-wire PostgreSQL workflows; run only through disposable fixture launchers."""
import os
from pathlib import Path
import tempfile
import unittest

from wire import Host


class PostgresWireTests(unittest.TestCase):
    def setUp(self):
        self.port = os.environ.get('DBVIEWER_TEST_PG_PORT')
        self.ca = os.environ.get('DBVIEWER_TEST_CA')
        if not self.port or not self.ca or not Path(self.ca).is_file():
            self.fail('Use a disposable PostgreSQL fixture launcher with its port and CA')
        self.tmp = tempfile.TemporaryDirectory(prefix='dbv-pg-wire-')
        self.addCleanup(self.tmp.cleanup)
        self.h = Host(self.tmp.name)
        self.addCleanup(self.h.close)

    def good(self, result):
        self.assertNotIn('error', result, result)

    def connect(self):
        h = self.h
        self.good(h.invoke('connect'))
        self.good(h.submit({'choice': 'PostgreSQL'}))
        self.good(h.submit({
            'name': 'fixture', 'host': 'localhost', 'port': self.port,
            'database': 'dbviewer', 'user': 'dbviewer', 'ca': self.ca,
            'plaintext': False, 'password-env': '', 'certificate': '', 'key': '',
        }))
        self.good(h.submit({'password': 'dbviewer-test-only'}))
        h.wait_jobs()
        self.assertTrue(all(state == 'succeeded' for state in h.jobs.values()), h.jobs)

    def writable_password_prompt(self):
        self.good(self.h.invoke('mode', self.h.active))
        self.good(self.h.submit({'choice': 'READ AND WRITE'}))
        self.good(self.h.submit({'confirmed': True}))
        self.assertEqual(next(iter(self.h.inputs.values()))['fields'][0]['kind'], 'secret')

    def test_stale_mode_password_does_not_reconnect_after_disconnect(self):
        self.connect()
        self.writable_password_prompt()
        h = self.h
        self.good(h.invoke('global-disconnect', h.active))
        jobs = dict(h.jobs)
        reply = h.submit({'password': 'dbviewer-test-only'})
        self.assertIn('Connection changed', reply['error']['message'])
        self.assertEqual(h.jobs, jobs)
        self.assertFalse(h.leases)
        self.good(h.invoke('open'))
        self.assertIn('disconnected', str(h.views[h.active]))

    def test_read_and_reviewed_write_with_explicit_rollback(self):
        self.connect()
        h = self.h
        self.good(h.invoke('query', h.active))
        buffer = next(reversed(h.buffers))
        h.buffers[buffer] = 'SELECT 42 AS answer'
        self.good(h.invoke('run', buffer=buffer))
        h.wait_jobs()
        self.assertEqual(h.views[h.active]['rows'][0]['cells'][0]['text'], '42')
        self.good(h.invoke('return', buffer=buffer))
        self.writable_password_prompt()
        self.good(h.submit({'password': 'dbviewer-test-only'}))
        h.wait_jobs()
        self.good(h.invoke('use', buffer=buffer))
        self.good(h.submit({'choice': 'fixture'}))
        h.buffers[buffer] = 'CREATE TEMP TABLE dbv_wire_rollback(value integer)'
        self.good(h.invoke('run', buffer=buffer))
        self.good(h.invoke('activate', h.active, ['0']))
        self.good(h.submit({'confirmed': True}))
        h.wait_jobs()
        self.assertTrue(h.leases)
        self.good(h.invoke('rollback', buffer=buffer))
        h.wait_jobs()
        self.assertFalse(h.leases)
        self.assertEqual(h.state['document']['data']['uncertain'], [])
        h.buffers[buffer] = "SELECT to_regclass('pg_temp.dbv_wire_rollback') IS NULL AS rolled_back"
        self.good(h.invoke('run', buffer=buffer))
        self.good(h.invoke('activate', h.active, ['0']))
        self.good(h.submit({'confirmed': True}))
        h.wait_jobs()
        self.assertEqual(h.views[h.active]['rows'][0]['cells'][0]['text'], 't')
        self.good(h.invoke('rollback', buffer=buffer))
        h.wait_jobs()
        self.assertFalse(h.leases)


if __name__ == '__main__':
    unittest.main()
