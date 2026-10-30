#!/usr/bin/env python3
"""Drive a running ``lumap --lab`` using only the Python standard library."""

import argparse
import datetime as dt
import json
import os
from pathlib import Path
import sys
import urllib.error
import urllib.request


def session_path():
    if os.environ.get("LOCALAPPDATA"):
        base = Path(os.environ["LOCALAPPDATA"])
    elif os.environ.get("XDG_DATA_HOME"):
        base = Path(os.environ["XDG_DATA_HOME"])
    else:
        base = Path.home() / ".local" / "share"
    return base / "lumap" / "lab" / "session.json"


class Lab:
    def __init__(self):
        path = session_path()
        try:
            session = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            raise SystemExit(f"Cannot read {path}; start lumap --lab first: {error}")
        self.url = session["url"].rstrip("/")
        self.token = session["token"]

    def request(self, path, body=None):
        data = None if body is None else json.dumps(body).encode("utf-8")
        request = urllib.request.Request(
            self.url + path,
            data=data,
            headers={"X-Lumap-Lab": self.token, "Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(request, timeout=920) as response:
                return response.read(), response.headers.get_content_type()
        except urllib.error.HTTPError as error:
            detail = error.read().decode("utf-8", "replace")
            raise SystemExit(f"lab answered {error.code}: {detail}")


def save(body, mime, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    if mime == "application/json":
        value = json.loads(body)
        destination.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    else:
        destination.write_bytes(body)
    print(destination)


def command_snap(lab, args):
    query = f"?settle_ms={args.settle_ms}&frames={args.frames}"
    body, mime = lab.request("/lab/camera.png" + query)
    save(body, mime, Path(args.output))


def command_act(lab, args):
    text = args.json
    if text.startswith("@"):
        text = Path(text[1:]).read_text(encoding="utf-8")
    body, _ = lab.request("/lab/act", json.loads(text))
    print(json.dumps(json.loads(body), indent=2))


def command_get(lab, args):
    body, mime = lab.request(args.path)
    if args.output:
        save(body, mime, Path(args.output))
    elif mime == "application/json":
        print(json.dumps(json.loads(body), indent=2))
    else:
        sys.stdout.buffer.write(body)


def command_experiment(lab, args):
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    run = Path(args.output) / f"{stamp}-{args.name}"
    run.mkdir(parents=True)
    metrics = {"experiment": args.name, "started": dt.datetime.now().isoformat()}
    for path, name in [
        ("/lab/state", "state.json"),
        ("/lab/camera.json", "camera.json"),
        ("/lab/camera.png?settle_ms=300&frames=3", "camera.png"),
        ("/lab/ui.png", "ui.png"),
    ]:
        body, mime = lab.request(path)
        save(body, mime, run / name)
        if mime == "application/json":
            metrics[name[:-5]] = json.loads(body)
    metrics["finished"] = dt.datetime.now().isoformat()
    (run / "metrics.json").write_text(json.dumps(metrics, indent=2) + "\n", encoding="utf-8")
    (run / "report.md").write_text(
        f"# Lab experiment: {args.name}\n\n"
        f"Run: `{stamp}`\n\n"
        "## Observations\n\n- Add findings after reviewing camera.png and ui.png.\n",
        encoding="utf-8",
    )
    print(run)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    snap = sub.add_parser("snap", help="save a settled median camera image")
    snap.add_argument("output", nargs="?", default="camera.png")
    snap.add_argument("--settle-ms", type=int, default=300)
    snap.add_argument("--frames", type=int, default=3)
    snap.set_defaults(run=command_snap)
    act = sub.add_parser("act", help="send an action array as JSON or @file")
    act.add_argument("json")
    act.set_defaults(run=command_act)
    get = sub.add_parser("get", help="read any GET endpoint")
    get.add_argument("path")
    get.add_argument("--output", "-o")
    get.set_defaults(run=command_get)
    experiment = sub.add_parser("experiment", help="capture a reviewable named run")
    experiment.add_argument("name")
    experiment.add_argument("--output", default="lab-runs")
    experiment.set_defaults(run=command_experiment)
    args = parser.parse_args()
    args.run(Lab(), args)


if __name__ == "__main__":
    main()
