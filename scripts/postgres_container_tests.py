# SPDX-License-Identifier: MPL-2.0
"""Run the PostgreSQL suites using an already installed Docker image on Linux.

Usage: python3 scripts/postgres_container_tests.py --image postgres:17
No image is pulled; all database state is temporary and the container is removed.
Native CI uses postgres_tests.py. This alternative also exercises Unix sockets.
"""
import argparse
import os
from pathlib import Path
import socket
import subprocess
import tempfile

from postgres_tests import ROOT, create_certificates


def run(command):
    result = subprocess.run([str(arg) for arg in command], capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f"Fixture command {command[0]} failed:\n{result.stderr}\n{result.stdout}")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="dbv-pg-", dir="/tmp") as directory:
        root = Path(directory)
        sockets = root / "socket"
        sockets.mkdir(mode=0o777)
        sockets.chmod(0o777)  # The container's postgres user owns its socket only.
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        create_certificates(root)
        (root / "password").write_text("dbviewer-test-only\n")
        (root / "password").chmod(0o600)
        (root / "postgresql.conf").write_text(
            f"listen_addresses='*'\nport={port}\nunix_socket_directories='/socket'\n"
            "ssl=on\nssl_ca_file='/certs/ca.crt'\n"
            "ssl_cert_file='/certs/server.crt'\nssl_key_file='/certs/server.key'\n")
        (root / "pg_hba.conf").write_text(
            "local all all trust\nhostssl all dbviewer_cert all cert\n"
            "host all all all scram-sha-256\n")
        container = None
        try:
            container = run([
                "docker", "create", "--pull=never", "--entrypoint", "sleep",
                "--tmpfs", "/data", "--tmpfs", "/var/lib/postgresql/data",
                "-v", f"{sockets}:/socket:Z", "-p", f"127.0.0.1:{port}:{port}",
                args.image, "infinity",
            ]).stdout.strip()
            run(["docker", "start", container])
            run(["docker", "exec", container, "mkdir", "/certs"])
            for name in ["ca.crt", "server.crt", "server.key", "password", "postgresql.conf", "pg_hba.conf"]:
                run(["docker", "cp", root / name, f"{container}:/certs/{name}"])
            run(["docker", "exec", container, "chown", "-R", "postgres:postgres", "/data", "/certs"])

            def postgres(*command):
                return run(["docker", "exec", "--user", "postgres", container, *command])

            postgres("initdb", "-D", "/data", "-U", "dbviewer", "--pwfile=/certs/password",
                     "--auth-local=trust", "--auth-host=scram-sha-256", "--no-locale", "--encoding=UTF8")
            postgres("pg_ctl", "-D", "/data", "-l", "/data/server.log", "-w", "start", "-o",
                     "-c config_file=/certs/postgresql.conf -c hba_file=/certs/pg_hba.conf")
            postgres("psql", "-h", "/socket", "-p", str(port), "-U", "dbviewer", "-d", "postgres",
                     "-c", "CREATE DATABASE dbviewer", "-c", "CREATE ROLE dbviewer_cert LOGIN")
            version = postgres("postgres", "--version").stdout.strip()
            print(f"Disposable fixture: {version} ({args.image})", flush=True)
            env = {**os.environ, "DBVIEWER_TEST_PG_PORT": str(port),
                   "DBVIEWER_TEST_CA": str(root / "ca.crt"),
                   "DBVIEWER_TEST_CLIENT_CERT": str(root / "client.crt"),
                   "DBVIEWER_TEST_CLIENT_KEY": str(root / "client.key"),
                   "DBVIEWER_TEST_SOCKET": str(sockets)}
            subprocess.run(["cargo", "test", "--locked", "--test", "databases", "postgres_",
                            "--", "--ignored"], cwd=ROOT, env=env, check=True)
            subprocess.run(["python3", "tests/postgres_wire.py"], cwd=ROOT, env=env, check=True)
        finally:
            if container:
                run(["docker", "rm", "--force", "--volumes", container])


if __name__ == "__main__":
    main()
