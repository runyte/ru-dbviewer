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

def main():
    with tempfile.TemporaryDirectory(prefix='dbv-pg-') as directory:
        root=Path(directory); data=root/'data';password=root/'password';password.write_text('dbviewer-test-only\n');password.chmod(0o600)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1',0));port=listener.getsockname()[1]
        run([pg('initdb'),'-D',data,'-U','dbviewer','--pwfile',password,'--auth-local=trust','--auth-host=scram-sha-256','--no-locale','--encoding=UTF8'])
        run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-keyout',root/'ca.key','-out',root/'ca.crt','-days','1','-subj','/CN=dbviewer-test-ca'])
        for name,subject,usage in [('server','localhost','serverAuth'),('client','dbviewer_cert','clientAuth')]:
            run(['openssl','req','-newkey','rsa:2048','-nodes','-keyout',root/f'{name}.key','-out',root/f'{name}.csr','-subj',f'/CN={subject}'])
            (root/f'{name}.ext').write_text(f'basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage={usage}\nsubjectAltName=DNS:{subject}\n')
            run(['openssl','x509','-req','-in',root/f'{name}.csr','-CA',root/'ca.crt','-CAkey',root/'ca.key','-CAcreateserial','-out',root/f'{name}.crt','-days','1','-extfile',root/f'{name}.ext'])
            (root/f'{name}.key').chmod(0o600)
        with (data/'postgresql.conf').open('a') as f:
            f.write(f"\nlisten_addresses='127.0.0.1'\nport={port}\nunix_socket_directories='{root}'\nssl=on\nssl_ca_file='{root}/ca.crt'\nssl_cert_file='{root}/server.crt'\nssl_key_file='{root}/server.key'\n")
        hba=data/'pg_hba.conf';hba.write_text('hostssl all dbviewer_cert 127.0.0.1/32 cert\n'+hba.read_text())
        started=False
        try:
            run([pg('pg_ctl'),'-D',data,'-l',root/'server.log','-w','start']);started=True
            run([pg('psql'),'-h',root,'-p',port,'-U','dbviewer','-d','postgres','-c','CREATE DATABASE dbviewer','-c','CREATE ROLE dbviewer_cert LOGIN'])
            env={**os.environ,'DBVIEWER_TEST_PG_PORT':str(port),'DBVIEWER_TEST_CA':str(root/'ca.crt'),'DBVIEWER_TEST_CLIENT_CERT':str(root/'client.crt'),'DBVIEWER_TEST_CLIENT_KEY':str(root/'client.key'),'DBVIEWER_TEST_SOCKET':str(root)}
            subprocess.run(['cargo','test','--locked','--test','databases','postgres_','--','--ignored'],cwd=ROOT,env=env,check=True)
        finally:
            if started:run([pg('pg_ctl'),'-D',data,'-m','immediate','-w','stop'])

if __name__=='__main__':main()
