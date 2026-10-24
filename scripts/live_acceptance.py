#!/usr/bin/env python3
"""Run isolated live acceptance against the already-started repository fixtures."""

import json
import os
from pathlib import Path
import queue
import re
import secrets
import shutil
import signal
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
PROJECT = "supabase-rs-utils-fixtures"
REALTIME = f"supabase_realtime_{PROJECT}"
SECRETS = set()
ENV = os.environ.copy()


def redact(text):
    for value in sorted(SECRETS, key=len, reverse=True):
        if value:
            text = text.replace(value, "[REDACTED]")
    text = re.sub(r"eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+", "[REDACTED JWT]", text)
    return re.sub(r"sb_(?:secret|publishable)_[A-Za-z0-9_-]+", "[REDACTED KEY]", text)


def stop(process):
    # The CLI may exit before its descendants; ownership is the process group.
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        process.wait(timeout=5)
        return
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait(timeout=5)


class Child:
    def __init__(self, argv, *, merge_stderr=True):
        self.process = subprocess.Popen(
            argv, cwd=ROOT, env=ENV, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT if merge_stderr else subprocess.DEVNULL,
            text=True, errors="replace", start_new_session=True,
        )
        self.lines = queue.Queue()
        self.reader = threading.Thread(target=self.drain, daemon=True)
        self.reader.start()

    def drain(self):
        try:
            for line in self.process.stdout:
                self.lines.put(line)
        finally:
            self.lines.put(None)

    def run(self, timeout, on_line=None, quiet=False):
        deadline = time.monotonic() + timeout
        captured = []
        try:
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise RuntimeError("child process timed out")
                try:
                    line = self.lines.get(timeout=min(remaining, 0.2))
                except queue.Empty:
                    continue
                if line is None:
                    break
                if quiet:
                    captured.append(line)
                else:
                    print(redact(line), end="", flush=True)
                if on_line:
                    on_line(line.strip())
            code = self.process.wait(timeout=max(0.1, deadline - time.monotonic()))
            if code:
                raise RuntimeError(f"child process failed with exit code {code}")
            return "".join(captured)
        finally:
            stop(self.process)


def command(argv, timeout=60, quiet=False):
    # JSON commands must not mix CLI deprecation warnings into their stdout.
    return Child(argv, merge_stderr=not quiet).run(timeout, quiet=quiet)


def http(base, path, key, body=None, method=None):
    request = urllib.request.Request(
        base + path, data=None if body is None else json.dumps(body).encode(),
        headers={"apikey": key, "Authorization": f"Bearer {key}",
                 "Content-Type": "application/json"}, method=method,
    )
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            data = response.read()
            return json.loads(data) if data else None
    except urllib.error.HTTPError as error:
        # Auth responses can contain credentials: never include their bodies.
        raise RuntimeError(f"HTTP {error.code} for {path.split('?')[0]}") from None


def test(package, target, name=None, on_line=None):
    argv = ["cargo", "test", "--locked", "--all-features", "-p", package, "--test", target]
    if name:
        argv.append(name)
    argv += ["--", "--ignored", "--nocapture", "--test-threads=1", "--format", "terse"]
    if name:
        argv.append("--exact")
    Child(argv).run(900, on_line=on_line)


def main():
    for dependency in ("cargo", "supabase", "docker"):
        if not shutil.which(dependency):
            raise RuntimeError(f"required dependency missing: {dependency}")
    if sys.version_info < (3, 11):
        raise RuntimeError("Python 3.11 or newer is required")
    import tomllib
    with (ROOT / "supabase/config.toml").open("rb") as source:
        config = tomllib.load(source)
    if config.get("project_id") != PROJECT or config.get("api", {}).get("port") != 56421:
        raise RuntimeError("repository fixture project/port configuration does not match the safety guard")
    try:
        status = json.loads(command(["supabase", "status", "-o", "json"], quiet=True))
    except (ValueError, RuntimeError):
        raise RuntimeError("cannot read fixture status; run supabase start from the repository first") from None
    for key, value in status.items():
        if isinstance(value, str) and any(word in key for word in ("KEY", "SECRET", "TOKEN", "PASSWORD", "DB_URL")):
            SECRETS.add(value)
    base = status.get("API_URL", "").rstrip("/")
    parsed = urllib.parse.urlparse(base)
    if parsed.scheme != "http" or parsed.hostname not in ("127.0.0.1", "localhost") or parsed.port != 56421:
        raise RuntimeError("refusing non-fixture API URL: expected local port 56421")
    # Inspect by exact name, not a substring match against all running stacks.
    inspected = json.loads(command(["docker", "inspect", REALTIME], quiet=True))
    if len(inspected) != 1 or inspected[0].get("Name") != "/" + REALTIME or not inspected[0].get("State", {}).get("Running"):
        raise RuntimeError("expected running fixture Realtime container is missing")
    before = inspected[0]["State"]["StartedAt"]
    for target, source in (("SUPABASE_ANON_KEY", "ANON_KEY"), ("SUPABASE_SERVICE_ROLE_KEY", "SERVICE_ROLE_KEY"), ("SUPABASE_MAILPIT_URL", "MAILPIT_URL")):
        if not status.get(source):
            raise RuntimeError(f"fixture status missing required {source}")
        ENV[target] = status[source]
    ENV["SUPABASE_URL"] = base
    anon, admin = ENV["SUPABASE_ANON_KEY"], ENV["SUPABASE_SERVICE_ROLE_KEY"]
    probe = {"left": 19, "right": 23}
    try:
        existing = http(base, "/functions/v1/product-probe", anon, probe)
    except (RuntimeError, urllib.error.URLError, TimeoutError):
        existing = None
    function = None
    user_id = None
    cleanup_errors = []
    try:
        if existing == {"sum": 42}:
            print("Using the fixture's running Edge Function.", flush=True)
        else:
            function = Child(["supabase", "functions", "serve", "product-probe"])
            deadline = time.monotonic() + 90
            while True:
                # Drain output while probing so the CLI cannot block on stdout.
                while True:
                    try:
                        line = function.lines.get_nowait()
                    except queue.Empty:
                        break
                    if line is not None:
                        print(redact(line), end="", flush=True)
                if function.process.poll() is not None:
                    raise RuntimeError("owned functions serve process exited before readiness")
                try:
                    if http(base, "/functions/v1/product-probe", anon, probe) == {"sum": 42}:
                        break
                except (RuntimeError, urllib.error.URLError, TimeoutError):
                    pass
                if time.monotonic() >= deadline:
                    raise RuntimeError("product-probe readiness timed out")
                time.sleep(0.5)
            def forward_function_output():
                while True:
                    line = function.lines.get()
                    if line is None:
                        return
                    print(redact(line), end="", flush=True)
            threading.Thread(target=forward_function_output, daemon=True).start()
        for target in ("live_core", "live_management", "live_pkce"):
            test("rp-supabase-auth", target)
        test("rp-supabase-storage", "product_gaps")
        test("rp-supabase-graphql", "live")
        test("supabase-rp", "live_product")
        test("rp-supabase-realtime", "live_session")
        password = secrets.token_urlsafe(32)
        SECRETS.add(password)
        email = f"live-reconnect-{secrets.token_hex(12)}@example.com"
        user = http(base, "/auth/v1/admin/users", admin,
                    {"email": email, "password": password, "email_confirm": True})
        user_id = user["id"]
        session = http(base, "/auth/v1/token?grant_type=password", anon,
                       {"email": email, "password": password})
        token = session["access_token"]
        SECRETS.update((token, session.get("refresh_token", "")))
        ENV["SUPABASE_ACCESS_TOKEN"] = token
        restarted = False

        def restart(line):
            nonlocal restarted
            if line == "REALTIME_READY_RESTART":
                if restarted:
                    raise RuntimeError("duplicate Realtime restart marker")
                command(["docker", "restart", REALTIME], timeout=60)
                after = json.loads(command(["docker", "inspect", REALTIME], quiet=True))[0]
                if not after["State"]["Running"] or after["State"]["StartedAt"] == before:
                    raise RuntimeError("Realtime container restart was not confirmed")
                restarted = True

        test("rp-supabase-realtime", "live_reconnect", on_line=restart)
        if not restarted:
            raise RuntimeError("Realtime test never requested a restart")
        if function and function.process.poll() is not None:
            raise RuntimeError("owned functions serve process exited during acceptance")
    finally:
        if user_id:
            try:
                http(base, f"/auth/v1/admin/users/{urllib.parse.quote(user_id, safe='')}", admin, method="DELETE")
            except Exception as error:
                cleanup_errors.append(f"fixture user deletion failed: {error}")
        if function:
            try:
                stop(function.process)
            except Exception as error:
                cleanup_errors.append(f"owned function process cleanup failed: {error}")
        if cleanup_errors:
            raise RuntimeError("; ".join(cleanup_errors))
    print("Live acceptance passed, including confirmed fixture Realtime restart and cleanup.")


def interrupted(_signum, _frame):
    raise KeyboardInterrupt("termination requested")


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, interrupted)
    try:
        main()
    except (Exception, KeyboardInterrupt) as error:
        print(redact(f"live acceptance failed: {error}"), file=sys.stderr)
        sys.exit(1)
