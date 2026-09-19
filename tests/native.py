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

def configuration():
    binary = Path(os.environ.get("DBVIEWER_BIN", Path(__file__).resolve().parents[1]/"target/debug/ru-dbviewer")).resolve()
    return json.loads(subprocess.check_output([str(binary), "--print-config"], text=True))


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
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.project = self.root / "project"
        self.project.mkdir()
        self.database = self.root / "tasks.sqlite3"
        config = configuration()
        config.update(lsp={"enable": False})
        with sqlite3.connect(self.database) as db:
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
                                RUNYTE_ALL_HOSTS_DIR=str(self.root / "all-hosts"))
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

    def wait_for(self, predicate):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            if predicate():
                return
            self.drain(0.1)
        self.test.fail(self.screen.text() if self.screen is not None else self.output[-8000:].decode(errors="replace"))

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

    def test_sqlite_browse_query_and_stop(self):
        with NativeEditor(self) as editor:
            self.connect(editor)
            editor.send(b"ggj\r")
            editor.wait_for(lambda: editor.shows("native-first"))
            editor.send(b"ggjj\r")
            editor.wait_for(lambda: editor.shows("name [TEXT]: native-first"))
            editor.command("db-query")
            editor.wait_for(lambda: editor.shows("SELECT 1;"))
            editor.command("db-run")
            editor.wait_for(lambda: editor.shows("rows retained") and editor.shows("READ ONLY"))
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
            editor.send(b"ggj\r")
            editor.wait_for(lambda: editor.shows("Execute reviewed SQL"))
            editor.send(b"\r")
            editor.wait_for(lambda: editor.shows("PENDING COMMIT"))
            self.assertEqual(editor.query("SELECT COUNT(*) FROM items")[0][0], 2)
            editor.command("db-commit")
            editor.wait_for(lambda: editor.shows("ready") and not editor.shows("PENDING COMMIT"))
            self.assertEqual(editor.query("SELECT name FROM items WHERE id=3"), [("native-write",)])

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
            editor.send(b"ggj\r")
            editor.wait_for(lambda: editor.shows("native-first"))
            editor.send(b"ggjj\r")
            editor.wait_for(lambda: editor.shows("name [TEXT]: native-first"))
            editor.send(b"-")
            editor.wait_for(lambda: editor.shows("page 1") and not editor.shows("name [TEXT]"))
            editor.send(b"\t")
            editor.wait_for(lambda: editor.shows("back"))
            editor.send(b"back\r")
            editor.wait_for(lambda: editor.shows("main.items"))
            editor.command("db-query")
            editor.wait_for(lambda: editor.shows("SELECT 1;"))
            self.assertFalse(list(editor.project.glob("*.sql")))
            editor.send(b"%cSELECT 42 AS unsaved_value;\x1b")
            editor.wait_for(lambda: editor.shows("SELECT 42 AS unsaved_value"))
            editor.command("db-run")
            editor.wait_for(lambda: editor.shows("unsaved_value") and editor.shows("42") and editor.shows("rows retained"))
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
            editor.wait_for(lambda: editor.shows("Databases"))
            editor.send(b"ggj\t")
            editor.wait_for(lambda: editor.shows("Application actions"))
            legacy = editor.shows("profile-actions")
            if os.environ.get("DBVIEWER_EXPECT_ROW_ACTIONS") == "1":
                self.assertFalse(legacy, editor.screen.text())
            if legacy:
                editor.send(b"profile-actions\r")
                editor.wait_for(lambda: editor.shows("disconnect"))
            self.assertTrue(re.search(r"\bdisconnect\b", editor.screen.text()))
            editor.send(b"disconnect\r")
            editor.wait_for(lambda: editor.shows("disconnected"))
            editor.send(b"\t")
            editor.wait_for(lambda: editor.shows("Application actions"))
            if legacy:
                editor.send(b"profile-actions\r")
                editor.wait_for(lambda: not editor.shows("Application actions"))
            self.assertFalse(re.search(r"\bdisconnect\b", editor.screen.text()), editor.screen.text())
            # Pick the exact action: the existing fuzzy menu can also match
            # connect-new, so typing its shared prefix is not a unique choice.
            for _ in range(8):
                if re.search(r"▸ connect\s", editor.screen.text()):
                    break
                editor.send(b"\x1b[B")
            self.assertRegex(editor.screen.text(), r"▸ connect\s")
            editor.send(b"\r")
            editor.wait_for(lambda: editor.shows("main.items") and editor.shows("ready"))

    def test_persistent_detach_and_reattach(self):
        with NativeEditor(self, persistent=True) as editor:
            self.connect(editor)
            editor.detach()
            editor.attach()
            editor.wait_for(lambda: editor.shows("main.items"))
            editor.send(b"ggj\r")
            editor.wait_for(lambda: editor.shows("native-first"))

if __name__ == "__main__":
    if not os.environ.get("RUNYTE_BIN"):
        raise SystemExit("Set RUNYTE_BIN to a supported real Runyte executable")
    unittest.main()
