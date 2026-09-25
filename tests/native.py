# SPDX-License-Identifier: MPL-2.0
"""Optional real-editor acceptance. Set RUNYTE_BIN to a compatible Runyte build."""
import codecs
import errno
import fcntl
from contextlib import closing
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import sqlite3
import struct
import subprocess
import tempfile
import termios
import time
import unicodedata
import unittest
from unittest.mock import patch

def configuration():
    binary = Path(os.environ.get("DBVIEWER_BIN", Path(__file__).resolve().parents[1]/"target/debug/ru-dbviewer")).resolve()
    config = json.loads(subprocess.check_output([str(binary), "--print-config"], text=True))
    if os.environ.get("DBVIEWER_EXPECT_DEFAULT_BINDINGS") == "1":
        config["plugins"][0].pop("bindings", None)
    return config


CONTROL = re.compile(rb"\x1b\[([0-?]*)[ -/]*([@-~])")
PARTIAL_CONTROL = re.compile(rb"\x1b(\[[0-?]*[ -/]*)?")
COMPLETED = "(Application command completed)"


class Screen:
    """The character most recently drawn in each cell of the editor's terminal.

    Ratatui writes only the cells that changed since its previous frame, so
    fresh output cannot show what is on screen: reopening a view that is
    already visible writes nothing at all. The editor positions every write,
    so cursor moves and clears are the only controls that change the cells.
    """
    def __init__(self, rows, columns):
        self.rows, self.columns = rows, columns
        self.cells = [[" "] * columns for _ in range(rows)]
        self.row = self.column = 0
        self.pending = b""
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")

    def feed(self, data):
        # A control sequence or a character may be split across reads.
        data, self.pending = self.pending + data, b""
        position = 0
        while position < len(data):
            escape = data.find(b"\x1b", position)
            self.write(self.decoder.decode(data[position:len(data) if escape < 0 else escape]))
            if escape < 0:
                break
            control = CONTROL.match(data, escape)
            if control is None:
                if PARTIAL_CONTROL.fullmatch(data, escape):
                    self.pending = data[escape:]
                    break
                position = escape + 2
                continue
            self.control(control.group(1), control.group(2))
            position = control.end()

    def write(self, text):
        for character in text:
            if character == "\r":
                self.column = 0
            elif character == "\n":
                self.row = min(self.row + 1, self.rows - 1)
            elif character >= " ":
                if self.column < self.columns:
                    self.cells[self.row][self.column] = character
                self.column += 2 if unicodedata.east_asian_width(character) in "WF" else 1

    def control(self, parameters, final):
        # Private modes (cursor visibility, keyboard flags) and colors leave cells alone.
        if not re.fullmatch(rb"[0-9;]*", parameters):
            return
        values = [int(value) if value else 0 for value in parameters.split(b";")]
        if final in (b"H", b"f"):
            row, column = (values + [0])[:2]
            self.row = min(max(row, 1), self.rows) - 1
            self.column = min(max(column, 1), self.columns) - 1
        elif final == b"J":
            start = 0 if values[0] in (2, 3) else self.row * self.columns + self.column
            for index in range(start, self.rows * self.columns):
                self.cells[index // self.columns][index % self.columns] = " "
        elif final == b"K":
            start = 0 if values[0] == 2 else self.column
            self.cells[self.row][start:] = [" "] * (self.columns - start)

    def text(self):
        return "\n".join("".join(row) for row in self.cells)


class NativeEditor:
    """A real PTY and optionally retained host, with all state below one temp root."""
    def __init__(self, test, *, persistent=False):
        self.test, self.persistent = test, persistent
        # Keep the canonical workspace socket path short on Linux and macOS.
        # Darwin's default /var/folders TMPDIR can exceed the host's limit once
        # our project/.runyte/host/workspace.sock suffix is appended.
        self.temporary = tempfile.TemporaryDirectory(prefix="dbv-", dir="/tmp")
        self.root = Path(self.temporary.name)
        self.project = self.root / "project"
        self.project.mkdir()
        self.database = self.root / "tasks.sqlite3"
        config = configuration()
        config.update(lsp={"enable": False})
        with closing(sqlite3.connect(self.database)) as db:
            db.executescript("CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO items VALUES(1, 'native-first'),(2, 'native-second');")
        self.config = self.root / "config" / "runyte" / "config.json"
        self.config.parent.mkdir(parents=True)
        self.config.write_text(json.dumps(config))
        (self.project / "note.txt").write_text("Native plugin acceptance\n")
        # Launch an independent editor even when the test command itself runs
        # inside a Runyte terminal. Do not inherit parent attachment, tracing,
        # shared host inventory, or internal test-control environment values.
        self.environment = {key: value for key, value in os.environ.items() if not key.startswith("RUNYTE_")}
        self.environment.update(TERM="xterm-256color", HOME=str(self.root / "home"),
                                RUNYTE_ALL_HOSTS_DIR=str(self.root / "all-hosts"), TMPDIR=str(self.root))
        for variable, name in [("XDG_DATA_HOME", "data"), ("XDG_CACHE_HOME", "cache"),
                               ("XDG_CONFIG_HOME", "config"), ("XDG_RUNTIME_DIR", "runtime"),
                               ("XDG_STATE_HOME", "state")]:
            self.environment[variable] = str(self.root / name)
        (self.root / "home").mkdir()
        self.output = bytearray()
        self.child = self.master = self.screen = None

    def attach(self):
        self.test.assertIsNone(self.master)
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 110, 0, 0))
        arguments = [os.environ["RUNYTE_BIN"], "--config", str(self.config),
                     "--persistent" if self.persistent else "--init", str(self.project)]
        try:
            self.child = subprocess.Popen(arguments, cwd=self.project, env=self.environment,
                                          stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
        except BaseException:
            os.close(master)
            raise
        finally:
            os.close(slave)
        self.master = master
        self.output.clear()
        self.screen = Screen(32, 110)
        self.wait_for(lambda: self.sees(b" NOR "))

    def sees(self, text):
        # Strip formatting from the observed output, including escape sequences
        # split across reads. These checks identify fresh, rendered status/command
        # text. Whether something is on screen now is a question for `shows`.
        return text in re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", self.output)

    def shows(self, text):
        return text in self.screen.text()

    def read_output(self, seconds):
        if not select.select([self.master], [], [], seconds)[0]:
            return True
        try:
            chunk = os.read(self.master, 65536)
        except OSError as error:
            # Linux reports EIO when the last slave closes; macOS returns EOF.
            if error.errno != errno.EIO:
                raise
            return False
        self.output.extend(chunk)
        if self.screen is not None:
            self.screen.feed(chunk)
        del self.output[:-65536]
        return bool(chunk)

    def drain(self, seconds=0.3):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if not self.read_output(max(0, min(0.05, end - time.monotonic()))):
                break
        self.test.assertIsNone(self.child.poll(), self.output[-8000:].decode(errors="replace"))

    def send(self, keys):
        os.write(self.master, keys)
        self.drain()

    def wait_for(self, predicate, timeout=8):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if predicate():
                return
            self.drain(0.1)
        self.test.fail(self.screen.text() if self.screen is not None else self.output[-8000:].decode(errors="replace"))

    def host_metrics(self):
        # Linux-only observations of this fixture's host, never a shared host.
        pid = self.child.pid
        if self.persistent:
            for endpoint in self.root.rglob("endpoint.json"):
                try:
                    pid = int(json.loads(endpoint.read_text())["pid"])
                    break
                except (FileNotFoundError, KeyError):
                    continue
        try:
            values = {}
            for line in Path(f"/proc/{pid}/status").read_text().splitlines():
                if line.startswith(("VmRSS:", "VmHWM:")):
                    key, value = line.split(":", 1)
                    values[key] = int(value.split()[0])
            stat = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
            values["cpu_ms"] = (int(stat[11]) + int(stat[12])) * 1000 / os.sysconf("SC_CLK_TCK")
            return values
        except (FileNotFoundError, PermissionError):
            return {}

    def query(self, sql):
        if not self.database.exists():
            return []
        with closing(sqlite3.connect(self.database)) as db:
            try:
                return db.execute(sql).fetchall()
            except sqlite3.OperationalError as error:
                if "no such table" in str(error):
                    return []
                raise

    def command(self, command):
        self.output.clear()
        self.send(("::"+command).encode())
        self.wait_for(lambda: self.sees(b"plugin.dbviewer."))
        self.send(b"\r")

    def find(self, text):
        # Search literal text in the active document; metadata rows are deliberately
        # not actionable and must not be navigated through fixed line counts.
        self.send(b"gg;s" + text.encode() + b"\r")

    def open_row(self, text, marker):
        self.find(text)
        self.send(b"\r")
        self.wait_for(lambda: self.shows(marker))

    def action(self, name):
        self.send(b"\t")
        self.wait_for(lambda: self.shows("Application actions"))
        self.send(name.encode() + b"\r")

    def present(self, keys, marker):
        # The host refuses to show a plugin's view, prompt or document once
        # later input has changed the foreground. Send nothing else until what
        # these keys ask for is on screen.
        self.test.assertFalse(self.shows(marker), self.screen.text())
        self.send(keys)
        self.wait_for(lambda: self.shows(marker))

    def wait_for_exit(self, timeout=5):
        # Keep the PTY sink active through the final screen/terminal restoration.
        # Darwin can wait for terminal output to drain before exposing child exit.
        deadline = time.monotonic() + timeout
        while self.child.poll() is None:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise subprocess.TimeoutExpired(self.child.args, timeout,
                                                output=bytes(self.output))
            if not self.read_output(min(0.05, remaining)):
                # EOF may arrive just before waitpid observes termination.
                time.sleep(min(0.01, remaining))
        return self.child.returncode

    def detach(self):
        os.write(self.master, b":detach\r")
        self.test.assertEqual(self.wait_for_exit(), 0)
        os.close(self.master)
        self.master = None

    def __enter__(self):
        try:
            self.attach()
        except BaseException as error:
            self.__exit__(type(error), error, error.__traceback__)
            raise
        return self

    def __exit__(self, exception_type, *_):
        # The stop acknowledgement precedes endpoint cleanup and plugin shutdown.
        # Capture fixture-owned hosts before stopping them, then wait for their
        # actual exit before deleting directories they can still write into.
        host_pids = set()
        if self.persistent:
            for endpoint in self.root.rglob("endpoint.json"):
                try:
                    host_pids.add(int(json.loads(endpoint.read_text())["pid"]))
                except FileNotFoundError:
                    pass
        try:
            try:
                # On failure, close the terminal sink before killing/reaping its
                # writer. An undrained master must not trap cleanup on macOS either.
                if self.master is not None:
                    os.close(self.master)
                    self.master = None
                if self.child is not None and self.child.poll() is None:
                    try:
                        os.killpg(self.child.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    self.child.wait(timeout=3)
            finally:
                if self.persistent:
                    # A detached host has a separate process group; explicitly stop
                    # only this isolated workspace before removing its state.
                    result = subprocess.run([os.environ["RUNYTE_BIN"], "--config", str(self.config),
                                             "--session-stop", str(self.project), "--force"],
                                            cwd=self.project, env=self.environment, capture_output=True,
                                            text=True, timeout=10)
                    deadline = time.monotonic() + 10
                    while host_pids:
                        for pid in list(host_pids):
                            process = subprocess.run(["ps", "-p", str(pid), "-o", "stat="],
                                                     capture_output=True, text=True, timeout=2)
                            state = process.stdout.strip()
                            if process.returncode == 1 or state.startswith("Z"):
                                host_pids.remove(pid)
                            elif process.returncode != 0:
                                raise RuntimeError(f"Cannot observe fixture host exit: {process.stderr}")
                        if host_pids:
                            if time.monotonic() >= deadline:
                                raise TimeoutError("Fixture persistent host did not exit after stop")
                            time.sleep(0.05)
                    if exception_type is None:
                        self.test.assertEqual(result.returncode, 0, result.stderr)
        finally:
            self.temporary.cleanup()


class NativeTests(unittest.TestCase):
    def connect(self, editor):
        editor.command("db-connect")
        editor.wait_for(lambda: editor.shows("Database type"))
        editor.send(b"\r")
        editor.wait_for(lambda: editor.shows("SQLite form"))
        editor.send(b"native\t"+str(editor.database).encode()+b"\r")
        editor.wait_for(lambda: editor.shows("main.items") and editor.shows("ready"))

    def test_postgres_connection_form_from_database_actions(self):
        for persistent in (False, True):
            with self.subTest(persistent=persistent), NativeEditor(self, persistent=persistent) as editor:
                editor.command("db")
                editor.wait_for(lambda: editor.shows("[databases]"))
                editor.send(b"\t")
                editor.wait_for(lambda: editor.shows("Application actions"))
                editor.send(b"connect-new\r")
                editor.wait_for(lambda: editor.shows("Database type"))
                editor.send(b"PostgreSQL\r")
                editor.wait_for(lambda: editor.shows("PostgreSQL connection"))
                self.assertTrue(editor.shows("Host or Unix socket directory"))
                # Submit required fields, retaining the default port and verified TLS.
                editor.send(b"native-pg\tlocalhost\t\tfixture\tfixture\r")
                editor.wait_for(lambda: editor.shows("PostgreSQL password"))
                editor.send(b"\x1b")
                editor.wait_for(lambda: not editor.shows("PostgreSQL password"))
                editor.send(b":notifications\r")
                editor.wait_for(lambda: editor.shows("[notifications]"))
                self.assertFalse(editor.shows("Plugin result rejected"), editor.screen.text())

    def test_sqlite_browse_query_and_stop(self):
        with NativeEditor(self) as editor:
            self.connect(editor)
            editor.open_row("main.items", "native-first")
            editor.open_row("native-first", "name [TEXT]: native-first")
            editor.command("db-query")
            editor.wait_for(lambda: editor.shows("SELECT 1;"))
            editor.command("db-run")
            editor.wait_for(lambda: editor.shows("[results]") and editor.shows("READ ONLY"))
            editor.send(b":plugin-stop dbviewer\r")
            editor.wait_for(lambda: editor.shows("unavailable") or editor.shows("stopped"))

    def test_native_write_review_and_commit(self):
        with NativeEditor(self) as editor:
            self.connect(editor)
            editor.send(b":plugin.dbviewer.mode\r")
            editor.wait_for(lambda: editor.shows("Access mode"))
            editor.send(b"READ AND WRITE\r")
            editor.wait_for(lambda: editor.shows("Enable READ AND WRITE"))
            editor.send(b"\r")
            editor.wait_for(lambda: editor.shows("WRITABLE") and editor.shows("main.items"))
            (editor.project/"write.sql").write_text("INSERT INTO items VALUES(3, 'native-write');\n")
            editor.send(b":open write.sql\r")
            editor.wait_for(lambda: editor.shows("INSERT INTO items"))
            editor.command("db-use")
            editor.wait_for(lambda: editor.shows("Database for this SQL buffer"))
            editor.send(b"\r")
            editor.command("db-run")
            editor.wait_for(lambda: editor.shows("captured SQL"))
            editor.open_row("INSERT INTO items", "Execute reviewed SQL")
            editor.send(b"\r")
            editor.wait_for(lambda: editor.shows("PENDING COMMIT"))
            self.assertEqual(editor.query("SELECT COUNT(*) FROM items")[0][0], 2)
            editor.command("db-commit")
            editor.wait_for(lambda: editor.shows("ready") and not editor.shows("PENDING COMMIT"))
            self.assertEqual(editor.query("SELECT name FROM items WHERE id=3"), [("native-write",)])

    def test_truncated_json_value_and_back(self):
        with NativeEditor(self) as editor:
            payload = json.dumps({"assignment": {"members": [
                {"id": i, "name": "member" * 50} for i in range(300)
            ]}}, separators=(",", ":"))
            with closing(sqlite3.connect(editor.database)) as db:
                db.execute("UPDATE items SET name=? WHERE id=1", (payload,))
                db.commit()
            self.connect(editor)
            editor.open_row("main.items", "[rows]")
            editor.open_row('{"assignment"', "name [TEXT]")
            editor.open_row("name [TEXT]", "indented JSON prefix")
            self.assertTrue(editor.shows('"assignment": {'))
            self.assertTrue(editor.shows('"members": ['))
            editor.send(b"\t")
            editor.wait_for(lambda: editor.shows("Application actions"))
            editor.send(b"raw\r")
            editor.wait_for(lambda: editor.shows('{"assignment":{"members":['))
            editor.send(b"-")
            editor.wait_for(lambda: editor.shows("name [TEXT]"))
            editor.send(b"-")
            editor.wait_for(lambda: editor.shows("[rows]") and not editor.shows("name [TEXT]"))

    @unittest.skipUnless(os.environ.get("DBVIEWER_EXPECT_VIEW_HELP") == "1",
                         "requires a host with view-help")
    def test_space_question_explains_the_current_page(self):
        with NativeEditor(self) as editor:
            self.connect(editor)
            editor.open_row("main.items", "[rows]")
            editor.present(b" ?", "DATABASE VIEWER · ROWS")
            self.assertTrue(editor.shows("one page of a table"), editor.screen.text())
            # The host lists the live menu below the prose, labels and groups
            # included; this row appears nowhere else.
            editor.find("Page size — Set browse page size")
            editor.wait_for(lambda: editor.shows("Columns and paging"))
            editor.send(b"q")
            editor.wait_for(lambda: editor.shows("[rows]"))
            editor.open_row("native-first", "[record]")
            editor.present(b" ?", "DATABASE VIEWER · RECORD")
            self.assertFalse(editor.shows("one page of a table"), editor.screen.text())

    @unittest.skipUnless(os.environ.get("DBVIEWER_EXPECT_PATH_COMPLETION") == "1",
                         "requires a host with input-path-completion")
    def test_native_live_path_completion_and_short_labels(self):
        for persistent in (False, True):
            with self.subTest(persistent=persistent), NativeEditor(self, persistent=persistent) as editor:
                editor.send(b":cd ..\r")
                editor.wait_for(lambda: editor.shows("working directory:"))
                editor.command("db-connect")
                editor.wait_for(lambda: editor.shows("Database type"))
                editor.send(b"\r")
                editor.wait_for(lambda: editor.shows("SQLite form"))
                self.assertTrue(editor.shows("Local SQLite file (required)"))
                editor.send(b"native\t../tas")
                editor.wait_for(lambda: editor.shows("tasks.sqlite3") and editor.shows("complete"))
                self.assertFalse(editor.shows("Resolved paths"))
                editor.send(b"\t")
                editor.wait_for(lambda: editor.shows("../tasks.sqlite3"))
                self.assertTrue(editor.shows("SQLite form"))
                editor.send(b"\r")
                editor.wait_for(lambda: editor.shows("main.items") and editor.shows("ready"))

    def test_native_completion_back_and_unsaved_sql(self):
        with NativeEditor(self) as editor:
            editor.command("db-connect")
            editor.wait_for(lambda: editor.shows("Database type"))
            editor.send(b"\r")
            editor.wait_for(lambda: editor.shows("SQLite form"))
            editor.send(b"native\t../tasks\r")
            editor.wait_for(lambda: editor.shows("Resolved paths"))
            editor.send(b"\r")
            editor.wait_for(lambda: editor.shows("main.items") and editor.shows("ready"))
            editor.open_row("main.items", "native-first")
            editor.open_row("native-first", "name [TEXT]: native-first")
            editor.send(b"-")
            editor.wait_for(lambda: editor.shows("[rows]") and not editor.shows("name [TEXT]"))
            editor.send(b"\t")
            editor.wait_for(lambda: editor.shows("Back") or editor.shows("back"))
            editor.send(b"back\r")
            editor.wait_for(lambda: editor.shows("main.items"))
            editor.command("db-query")
            editor.wait_for(lambda: editor.shows("SELECT 1;"))
            self.assertFalse(list(editor.project.glob("*.sql")))
            editor.send(b"%cSELECT 42 AS unsaved_value;\x1b")
            editor.wait_for(lambda: editor.shows("SELECT 42 AS unsaved_value"))
            editor.command("db-run")
            editor.wait_for(lambda: editor.shows("unsaved_value") and editor.shows("42") and editor.shows("[results]"))
            self.assertFalse(list(editor.project.glob("*.sql")))
            editor.send(b"\x1bo")
            editor.wait_for(lambda: editor.shows("SELECT 42 AS unsaved_value"))
            editor.send(b":w\r")
            editor.wait_for(lambda: bool(list(editor.project.glob("*.sql"))))
            self.assertIn("SELECT 42 AS unsaved_value",next(editor.project.glob("*.sql")).read_text())
            editor.command("db-return")
            editor.wait_for(lambda: editor.shows("main.items"))

    def test_selected_profile_disconnect_and_reconnect_actions(self):
        with NativeEditor(self) as editor:
            self.connect(editor)
            editor.command("db")
            editor.wait_for(lambda: editor.shows("[databases]"))
            editor.find("native ·")
            editor.send(b"\t")
            editor.wait_for(lambda: editor.shows("Application actions"))
            legacy = editor.shows("profile-actions")
            if os.environ.get("DBVIEWER_EXPECT_ROW_ACTIONS") == "1":
                self.assertFalse(legacy, editor.screen.text())
            if legacy:
                editor.send(b"profile-actions\r")
                editor.wait_for(lambda: editor.shows("Disconnect") or editor.shows("disconnect"))
            self.assertTrue(re.search(r"\bdisconnect\b", editor.screen.text(), re.IGNORECASE))
            editor.send(b"disconnect\r")
            editor.wait_for(lambda: editor.shows("disconnected"))
            editor.send(b"\t")
            editor.wait_for(lambda: editor.shows("Application actions"))
            if legacy:
                editor.send(b"profile-actions\r")
                editor.wait_for(lambda: not editor.shows("Application actions"))
            self.assertFalse(re.search(r"\bdisconnect\b", editor.screen.text(), re.IGNORECASE), editor.screen.text())
            # Pick the exact action: the existing fuzzy menu can also match
            # connect-new, so typing its shared prefix is not a unique choice.
            for _ in range(8):
                if re.search(r"▸ [Cc]onnect\s", editor.screen.text()):
                    break
                editor.send(b"\x1b[B")
            self.assertRegex(editor.screen.text(), r"▸ [Cc]onnect\s")
            editor.send(b"\r")
            editor.wait_for(lambda: editor.shows("main.items") and editor.shows("ready"))

    def large_value(self, editor):
        value = {"members": [{"id": i, "name": "member" * 65} for i in range(12000)],
                 "tail": "NATIVE_FULL_TAIL_573"}
        raw = json.dumps(value, separators=(",", ":"))
        self.assertGreater(len(raw.encode()), 4 * 1024 * 1024)
        with closing(sqlite3.connect(editor.database)) as db:
            db.execute("UPDATE items SET name=? WHERE id=1", (raw,))
            db.commit()
        self.connect(editor)
        editor.open_row("main.items", "[rows]")
        editor.open_row('{"members"', "name [TEXT]")
        editor.open_row("name [TEXT]", "[value]")
        self.assertFalse(editor.shows("NATIVE_FULL_TAIL_573"))
        return value, raw

    @unittest.skipUnless(os.environ.get("DBVIEWER_EXPECT_FULL_VALUES") == "1",
                         "requires current host view-document and job-feedback features")
    def test_full_value_search_copy_raw_back_and_persistent_reattach(self):
        with NativeEditor(self, persistent=True) as editor:
            value, raw = self.large_value(editor)
            editor.send(b"\t")
            editor.wait_for(lambda: editor.shows("Application actions"))
            self.assertTrue(editor.shows("Inspect"), editor.screen.text())
            self.assertTrue(editor.shows("Show full value"), editor.screen.text())
            self.assertNotRegex(editor.screen.text(), r"\bActivate\b")
            before = editor.host_metrics()
            started = time.monotonic()
            editor.send(b"show-full\r")
            editor.wait_for(lambda: editor.shows("Loading full value") or editor.shows("Complete ·"), timeout=30)
            input_during_load = editor.shows("Loading full value")
            input_started = time.monotonic()
            os.write(editor.master, b":")
            editor.wait_for(lambda: editor.shows(" CMD "))
            input_seconds = time.monotonic() - input_started
            editor.send(b"\x1b")
            editor.wait_for(lambda: editor.shows("Complete ·") and editor.shows('"members": ['), timeout=60)
            load_seconds = time.monotonic() - started
            loaded = editor.host_metrics()
            editor.find("NATIVE_FULL_TAIL_573")
            editor.wait_for(lambda: editor.shows('"tail": "NATIVE_FULL_TAIL_573"'))
            # Native search reaches past the old 4 MiB/10,000-row limits. Ordinary
            # whole-buffer yank and paste exercise every internal transport chunk.
            editor.send(b"%y")
            copied = editor.project / "copied-value.txt"
            copied.write_text("")
            editor.send(b":open copied-value.txt\r")
            editor.wait_for(lambda: editor.shows("copied-value.txt"))
            editor.send(b"p:w\r")
            editor.wait_for(lambda: copied.stat().st_size > 4 * 1024 * 1024, timeout=30)
            text = copied.read_text()
            body = text[text.index("{"):]
            formatted_lines = body.count("\n") + 1
            self.assertGreater(formatted_lines, 10000)
            self.assertEqual(json.loads(body), value)
            editor.send(b"\x1bo")
            editor.wait_for(lambda: editor.shows("[value]"))
            editor.send(b"gg")
            editor.action("raw")
            editor.wait_for(lambda: editor.shows("Complete ·") and editor.shows('{"members":['), timeout=60)
            editor.find("NATIVE_FULL_TAIL_573")
            editor.wait_for(lambda: editor.shows("NATIVE_FULL_TAIL_573"))
            editor.detach()
            editor.attach()
            editor.wait_for(lambda: editor.shows("NATIVE_FULL_TAIL_573"), timeout=30)
            # Copy raw mode too: no SQL replay or pretty-print spelling can replace it.
            editor.send(b"%y")
            raw_copy = editor.project / "copied-raw.txt"
            raw_copy.write_text("")
            editor.send(b":open copied-raw.txt\r")
            editor.wait_for(lambda: editor.shows("copied-raw.txt"))
            editor.send(b"p:w\r")
            editor.wait_for(lambda: raw_copy.stat().st_size > 4 * 1024 * 1024, timeout=30)
            copied_text = raw_copy.read_text()
            self.assertEqual(copied_text[copied_text.index('{"members":'):], raw)
            editor.send(b"\x1bo")
            editor.wait_for(lambda: editor.shows("[value]"))
            editor.send(b"gg;")
            editor.drain(2)
            idle_before = editor.host_metrics()
            editor.drain(1)
            idle_after = editor.host_metrics()
            idle_cpu_ms = (idle_after["cpu_ms"] - idle_before["cpu_ms"]
                           if "cpu_ms" in idle_before and "cpu_ms" in idle_after else None)
            print(f"Native debug full-value observation: raw={len(raw.encode())} bytes, "
                  f"formatted={len(body.encode())} bytes/{formatted_lines} lines, "
                  f"observed_harness_load={load_seconds:.3f}s, "
                  f"observed_harness_command_prompt={input_seconds:.3f}s "
                  f"(during_load={input_during_load}; send drain0.3s, wait polling0.1s), "
                  f"host_only_before={before}, host_only_loaded={loaded}, "
                  f"host_only_after_copy={idle_after} (VmRSS/VmHWM in KiB), "
                  f"settled_full_document_host_cpu_ms_per_second={idle_cpu_ms}", flush=True)
            editor.send(b"-")
            editor.wait_for(lambda: editor.shows("[record]") and editor.shows("name [TEXT]"))
            # The field selection is restored: Enter immediately reopens its preview.
            editor.send(b"\r")
            editor.wait_for(lambda: editor.shows("[value]") and editor.shows("Preview"))
            editor.action("back")
            editor.wait_for(lambda: editor.shows("[record]") and editor.shows("name [TEXT]"))
            editor.send(b"-")
            editor.wait_for(lambda: editor.shows("[rows]") and not editor.shows("name [TEXT]"))

    @unittest.skipUnless(os.environ.get("DBVIEWER_EXPECT_FULL_VALUES") == "1",
                         "requires current host view-document and job-feedback features")
    def test_full_value_cancel_keeps_editor_responsive(self):
        with NativeEditor(self) as editor:
            self.large_value(editor)
            editor.action("show-full")
            editor.command("db-cancel")
            # A commit already installed is the successful atomic winner. Otherwise
            # cancellation restores a readable preview; neither path abandons the UI.
            editor.wait_for(lambda: editor.shows("cancelled") or editor.shows("Complete ·"), timeout=30)
            editor.send(b"gg")
            editor.action("back")
            editor.wait_for(lambda: editor.shows("[record]") and editor.shows("name [TEXT]"))

    def test_persistent_detach_and_reattach(self):
        # macOS runners have long temporary paths. Exercise that constraint on
        # every platform, including Python's cached choice of temporary directory.
        with tempfile.TemporaryDirectory() as directory:
            long_temp = Path(directory) / ("long-temp-" * 12)
            long_temp.mkdir()
            with patch.dict(os.environ, TMPDIR=str(long_temp)), patch("tempfile.tempdir", str(long_temp)):
                with NativeEditor(self, persistent=True) as editor:
                    self.connect(editor)
                    editor.detach()
                    editor.attach()
                    editor.wait_for(lambda: editor.shows("main.items"))
                    editor.open_row("main.items", "native-first")

if __name__ == "__main__":
    if not os.environ.get("RUNYTE_BIN"):
        raise SystemExit("Set RUNYTE_BIN to a supported real Runyte executable")
    unittest.main()
