# Shared by the confit vars and lookup plugins (controller side only).
#
# Ansible templates task args in a forked worker per task, so a cache in the
# worker dies with it. Instead the vars plugin, which runs in the main
# process, starts a small server on a private unix socket; workers ask it for
# sections, and each section is resolved by confit at most once per run.
# Values stay in memory, never on disk.

import atexit
import json
import os
from pathlib import Path
import shutil
import socket
import socketserver
import subprocess
import tempfile
import threading
import tomllib

import yaml
from ansible.errors import AnsibleError

# confit's own settings, not config.
SKIP = {"providers"}
SOCKET_ENV = "CONFIT_ANSIBLE_SOCKET"


def find_root(*starts):
    """The nearest directory holding confit.toml, walking up from each start."""
    for start in starts:
        start = Path(start).resolve()
        for directory in [start, *start.parents]:
            if (directory / "confit.toml").is_file():
                return directory
    return None


def sections(root):
    """confit.toml's top-level config sections."""
    with open(Path(root) / "confit.toml", "rb") as f:
        return [name for name in tomllib.load(f) if name not in SKIP]


def section(root, name):
    """One section, revealed, as Python data."""
    path = os.environ.get(SOCKET_ENV)
    # Forked workers inherit _server too, so check the pid, not just _server.
    if path and os.getpid() != _server_pid:
        return _ask(path, root, name)
    return _cache.get(str(root), name)


class _Cache:
    def __init__(self):
        self.values = {}
        self.locks = {}
        self.lock = threading.Lock()

    def get(self, root, name):
        key = (root, name)
        with self.lock:
            lock = self.locks.setdefault(key, threading.Lock())
        # Per-section lock: concurrent asks for one section resolve it once.
        with lock:
            if key not in self.values:
                self.values[key] = _show(root, name)
            return self.values[key]


_cache = _Cache()
_server = None
_server_pid = None


def _show(root, name):
    if shutil.which("confit") is None:
        raise AnsibleError(f"confit: needed for confit.{name}, but not on PATH")
    proc = subprocess.run(
        ["confit", "show", "--yaml", "--reveal", name],
        cwd=root, capture_output=True, text=True,
    )
    if proc.returncode != 0:
        raise AnsibleError(f"confit: {name}: {proc.stderr.strip()}")
    return yaml.safe_load(proc.stdout)


class _Handler(socketserver.StreamRequestHandler):
    def handle(self):
        request = json.loads(self.rfile.readline())
        try:
            reply = {"value": _cache.get(request["root"], request["name"])}
        except Exception as e:
            reply = {"error": str(e)}
        self.wfile.write(json.dumps(reply, default=str).encode())


class _Server(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    daemon_threads = True


def serve():
    """Start the run's section server, once, in the main process."""
    global _server, _server_pid
    if _server is not None or os.environ.get(SOCKET_ENV):
        return
    directory = tempfile.mkdtemp(prefix="confit-ansible-")  # 0700
    path = os.path.join(directory, "sock")
    _server = _Server(path, _Handler)
    _server_pid = os.getpid()
    threading.Thread(target=_server.serve_forever, daemon=True).start()
    atexit.register(shutil.rmtree, directory, True)
    # Inherited by the forked workers.
    os.environ[SOCKET_ENV] = path


def _ask(path, root, name):
    with socket.socket(socket.AF_UNIX) as conn:
        conn.connect(path)
        conn.sendall(json.dumps({"root": str(root), "name": name}).encode() + b"\n")
        conn.shutdown(socket.SHUT_WR)
        reply = json.loads(conn.makefile().read())
    if "error" in reply:
        raise AnsibleError(reply["error"])
    return reply["value"]
