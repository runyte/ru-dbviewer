# SPDX-License-Identifier: MPL-2.0
"""Run Rust integration tests against a disposable native PostgreSQL cluster.
Requires initdb/pg_ctl/psql (PG_BIN may name their directory), openssl and Cargo.
No user database, configuration or service is used. Run as an ordinary user.
"""
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
def run(args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, capture_output=True, text=True, **kwargs)
def pg(name):
    return str(Path(os.environ['PG_BIN'])/name) if os.environ.get('PG_BIN') else shutil.which(name) or name

def openssl_environment(root):
    return {**os.environ, 'OPENSSL_CONF': str(root / 'openssl.cnf'),
            'OPENSSL_CONF_INCLUDE': str(root)}

def test_environment(root, port, sockets):
    # Keep the fixture CA explicit, so untrusted-root tests stay meaningful.
    roots_file = root / 'empty-roots.pem'
    roots_file.write_bytes(b'')
    roots_dir = root / 'empty-roots'
    roots_dir.mkdir(exist_ok=True)
    return {**os.environ, 'SSL_CERT_FILE': str(roots_file),
            'SSL_CERT_DIR': str(roots_dir), 'DBVIEWER_TEST_PG_PORT': str(port),
            'DBVIEWER_TEST_CA': str(root / 'ca.crt'),
            'DBVIEWER_TEST_CLIENT_CERT': str(root / 'client.crt'),
            'DBVIEWER_TEST_CLIENT_KEY': str(root / 'client.key'),
            'DBVIEWER_TEST_SOCKET': str(sockets)}

class NativeCluster:
    def __enter__(self):
        self.root = Path(tempfile.mkdtemp(prefix='dbv-pg-'))
        self.start_attempted = False
        return self

    def start(self):
        # A failed wait does not establish that PostgreSQL failed to start.
        self.start_attempted = True
        run([pg('pg_ctl'), '-D', self.root / 'data', '-l', self.root / 'server.log',
             '-t', '60', '-w', 'start'], env=openssl_environment(self.root))

    def stop(self):
        try:
            run([pg('pg_ctl'), '-D', self.root / 'data', '-m', 'immediate',
                 '-t', '60', '-w', 'stop'], env=openssl_environment(self.root))
        except subprocess.CalledProcessError as stop_error:
            try:
                run([pg('pg_ctl'), '-D', self.root / 'data', 'status'],
                    env=openssl_environment(self.root))
            except subprocess.CalledProcessError as status_error:
                if status_error.returncode == 3:  # Documented: no server running.
                    return
            raise RuntimeError('Could not confirm PostgreSQL fixture shutdown') from stop_error

    def __exit__(self, error_type, error, traceback):
        try:
            if self.start_attempted:
                self.stop()
            shutil.rmtree(self.root)
        except Exception as cleanup_error:
            message = f'PostgreSQL fixture cleanup failed; retained storage at {self.root}'
            if error is None:
                raise RuntimeError(message) from cleanup_error
            error.add_note(message)
        return False

def create_database(root, port):
    # libpq's hostaddr and service settings are independent of psql's explicit
    # host argument. Clear every PG option, including future libpq additions.
    env = {key: value for key, value in openssl_environment(root).items()
           if not key.startswith('PG')}
    passfile = root / 'pgpass'
    passfile.write_text('')
    passfile.chmod(0o600)
    env.update(HOME=str(root), PGPASSFILE=str(passfile),
               PGSERVICEFILE=str(root / 'pg_service.conf'), PGSYSCONFDIR=str(root),
               PGSSLMODE='disable', PGGSSENCMODE='disable', PGCONNECT_TIMEOUT='5')
    return run([pg('psql'), '-X', '-w', '-h', root, '-p', port, '-U', 'dbviewer',
                '-d', 'postgres', '-c', 'CREATE DATABASE dbviewer',
                '-c', 'CREATE ROLE dbviewer_cert LOGIN'], env=env)

def create_certificates(root):
    config = root / 'openssl.cnf'
    config.write_text(
        '[req]\ndistinguished_name=subject\n[subject]\n'
        '[fixture_ca]\nbasicConstraints=critical,CA:TRUE\n'
        'keyUsage=critical,keyCertSign,cRLSign\n'
        'subjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid:always\n')
    env = openssl_environment(root)

    def openssl(*args):
        return run(['openssl', *args], env=env)

    openssl('req', '-x509', '-newkey', 'rsa:2048', '-nodes',
            '-keyout', root / 'ca.key', '-out', root / 'ca.crt',
            '-days', '1', '-subj', '/CN=dbviewer-test-ca', '-extensions', 'fixture_ca')
    for name, subject, usage in [('server', 'localhost', 'serverAuth'),
                                 ('client', 'dbviewer_cert', 'clientAuth')]:
        openssl('req', '-newkey', 'rsa:2048', '-nodes',
                '-keyout', root / f'{name}.key', '-out', root / f'{name}.csr',
                '-subj', f'/CN={subject}')
        (root / f'{name}.ext').write_text(
            'basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\n'
            f'extendedKeyUsage={usage}\nsubjectAltName=DNS:{subject}\n')
        openssl('x509', '-req', '-in', root / f'{name}.csr', '-CA', root / 'ca.crt',
                '-CAkey', root / 'ca.key', '-CAcreateserial', '-out', root / f'{name}.crt',
                '-days', '1', '-extfile', root / f'{name}.ext')
        (root / f'{name}.key').chmod(0o600)

def main():
    with NativeCluster() as cluster:
        root=cluster.root; data=root/'data';password=root/'password';password.write_text('dbviewer-test-only\n');password.chmod(0o600)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1',0));port=listener.getsockname()[1]
        create_certificates(root)
        run([pg('initdb'),'-D',data,'-U','dbviewer','--pwfile',password,'--auth-local=trust','--auth-host=scram-sha-256','--no-locale','--encoding=UTF8'], env=openssl_environment(root))
        with (data/'postgresql.conf').open('a') as f:
            f.write(f"\nlisten_addresses='127.0.0.1'\nport={port}\nunix_socket_directories='{root}'\nssl=on\nssl_ca_file='{root}/ca.crt'\nssl_cert_file='{root}/server.crt'\nssl_key_file='{root}/server.key'\n")
        hba=data/'pg_hba.conf';hba.write_text('hostssl all dbviewer_cert 127.0.0.1/32 cert\n'+hba.read_text())
        cluster.start()
        create_database(root, port)
        env = test_environment(root, port, root)
        subprocess.run(['cargo','test','--locked','--test','databases','postgres_','--','--ignored'],cwd=ROOT,env=env,check=True)
        subprocess.run(['python3', 'tests/postgres_wire.py'], cwd=ROOT, env=env, check=True)

if __name__=='__main__':main()
