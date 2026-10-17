#!/usr/bin/env python3
import argparse
import hashlib
import io
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
from urllib.request import urlopen
from zipfile import ZipFile

ROOT = Path(__file__).resolve().parents[1]
REVISION = "21b7b40c4b4eae96c8f7f53a3d03859a899a1d2e"
URL = f"https://codeload.github.com/OpenRotMG/Open-RotMG-Client/zip/{REVISION}"
SHA256 = "61f539f52f2634247b703746f0d50bb239a09730c91e73e61012c15d863142e6"
LOG = ROOT / ".local/server.log"


def prepare_client():
    cache = LOG.parent / f"client-{REVISION}.zip"
    cache.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if cache.exists():
        data = cache.read_bytes()
    else:
        print(f"Downloading local client: {URL}", flush=True)
        try:
            with urlopen(URL, timeout=60) as response:
                data = response.read()
        except Exception as error:
            raise RuntimeError(f"Download {URL} failed: {type(error).__name__}: {error}") from error
    actual = hashlib.sha256(data).hexdigest()
    if actual != SHA256:
        raise RuntimeError(f"Client archive checksum mismatch: expected {SHA256}, got {actual}. Remove {cache} and rerun.")
    prefix = f"Open-RotMG-Client-{REVISION}"
    client = cache.parent / prefix
    with ZipFile(io.BytesIO(data)) as archive:
        for item in archive.infolist():
            path = Path(item.filename)
            if path.is_absolute() or ".." in path.parts or path.parts[0] != prefix or (item.external_attr >> 16) & 0o170000 not in (0, 0o100000, 0o040000):
                raise RuntimeError(f"Unsafe client archive entry: {item.filename}")
        if not client.exists():
            stage = cache.parent / f"client-extract-{REVISION}"
            stage.mkdir(mode=0o700)
            archive.extractall(stage)
            (stage / prefix).rename(client)
            stage.rmdir()
        for name in ["client.swf", "index.html", "scripts/run.py", "run.sh"]:
            path = client / name
            if path.is_symlink() or not path.is_file() or path.read_bytes() != archive.read(f"{prefix}/{name}"):
                raise RuntimeError(f"Cached client asset is missing or modified: {path}. Remove {client} and rerun.")
        # ZipFile drops Unix permissions; restore the verified launcher on fresh and cached installs.
        (client / "run.sh").chmod((archive.getinfo(f"{prefix}/run.sh").external_attr >> 16) & 0o777)
    if not cache.exists():
        cache.write_bytes(data)
    return client


def run(command, cwd, log):
    with subprocess.Popen(command, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True) as process:
        try:
            for line in process.stdout:
                print(line, end="", flush=True)
                log.write(line)
                log.flush()
            return process.wait()
        except KeyboardInterrupt:
            process.send_signal(signal.SIGINT)
            process.wait()
            raise


def port_number(value):
    try:
        port = int(value)
    except ValueError:
        raise argparse.ArgumentTypeError("port must be an integer between 1 and 65535") from None
    if not 1 <= port <= 65535:
        raise argparse.ArgumentTypeError("port must be between 1 and 65535")
    return port


def main():
    parser = argparse.ArgumentParser(prog="run.sh", description="Download the client and start your local OpenRotMG game.")
    parser.add_argument("port", type=port_number, help="Local server port")
    parser.add_argument("--client", "--local-client", dest="local_client", type=Path, metavar="DIR", help="Use your own client checkout instead of downloading the pinned client")
    states = parser.add_mutually_exclusive_group()
    states.add_argument("--state", type=Path, default=ROOT / ".local", metavar="DIR", help="Isolate saves, keys, client cache and logs in this directory")
    states.add_argument("--test-state", type=Path, metavar="DIR", help="Create new PREPARED test state with local controls; refuses an existing game database")
    parser.add_argument("--dev-control", action="store_true", help="Enable loopback-only QA control")
    bots = parser.add_mutually_exclusive_group()
    bots.add_argument("--shore-bots", action="store_true", help="Enable the three default autonomous Wizards explicitly")
    bots.add_argument("--no-shore-bots", action="store_true", help="Disable autonomous Wizards for isolated testing")
    parser.add_argument("--bot-llm", action="store_true", help="Enable Nemotron social dialogue; requires AI_GATEWAY_API_KEY")
    parser.add_argument("--release", action="store_true", help="Build optimized gameplay")
    args = parser.parse_args()
    if shutil.which("cargo") is None:
        parser.error("Cargo not found. Install Rust/Cargo from https://rustup.rs/ and rerun.")
    global LOG
    state = (args.test_state if args.test_state is not None else args.state).resolve()
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    LOG = state / "server.log"
    with LOG.open("a") as log:
        try:
            try:
                with socket.socket() as probe:
                    probe.bind(("127.0.0.1", args.port))
            except OSError as error:
                raise RuntimeError(f"Cannot listen on 127.0.0.1:{args.port}: {error}") from error
            revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
            dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip())
            client = args.local_client.resolve() if args.local_client is not None else prepare_client()
            prepare = client / "scripts/run.py"
            if not prepare.is_file():
                raise RuntimeError(f"Missing client launcher: {prepare}. Use a complete Open-RotMG-Client checkout.")
            receipt = f"Local source: {revision} dirty={dirty}; port={args.port}; state={state}; client={client}"
            print(receipt, flush=True)
            log.write(receipt + "\n")
            log.flush()
            print(f"Preparing local client. Logs: {LOG}", flush=True)
            code = run([sys.executable, str(prepare), "--prepare-only"], client, log)
            if code:
                raise RuntimeError(f"Client preparation exited with status {code}. See the error above and {LOG}.")
            print(f"Building and starting your local game. Logs: {LOG}", flush=True)
            command = ["cargo", "run", "--locked"]
            if args.release:
                command.append("--release")
            command += ["--", "--client", str(client), "--port", str(args.port), "--test-state" if args.test_state is not None else "--state", str(state)]
            if args.dev_control:
                command.append("--dev-control")
            if args.shore_bots:
                command.append("--shore-bots")
            if args.no_shore_bots:
                command.append("--no-shore-bots")
            if args.bot_llm:
                command.append("--bot-llm")
            code = run(command, ROOT, log)
            if code:
                raise RuntimeError(f"Server exited with status {code}. See the error above and {LOG}.")
            return 0
        except KeyboardInterrupt:
            print("Local server startup interrupted.", file=sys.stderr)
            return 130
        except Exception as error:
            message = f"Local server failed: {type(error).__name__}: {error}"
            log.write(message + "\n")
            print(message, file=sys.stderr)
            print(f"Log: {LOG}", file=sys.stderr)
            return 1


if __name__ == "__main__":
    raise SystemExit(main())
