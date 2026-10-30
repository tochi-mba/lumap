# The lumap lab

Start `lumap --lab` to open the normal editor with a local automation service. It listens only
on `127.0.0.1`, rejects browser-originated requests, and requires the random token written to
the platform data folder under `lumap/lab/session.json`. The token and camera images stay on the
machine.

The standard-library client reads that session automatically:

```text
python scripts/lab/lab.py snap camera.png --frames 5
python scripts/lab/lab.py act '[{"do":"step","step":2}]'
python scripts/lab/lab.py get /lab/state
python scripts/lab/lab.py experiment scan-quality
```

An experiment creates `lab-runs/<time>-<name>/` with the camera, editor, state, metrics and a
report ready for observations. This folder is ignored by Git. Use `act @actions.json` for a long
action list.

## Endpoints

- `GET /lab/ping` reports whether the server and editor are responsive.
- `GET /lab/state` returns the editor, show, outputs, selections, readiness, scan and frame-rate
  state. `POST /lab/act` runs one or more editor actions.
- `GET /lab/camera.json` returns camera details. `GET /lab/camera.png?settle_ms=300&frames=3`
  returns a settled per-pixel median. `POST /lab/record` accepts `{"seconds": 5}`.
- `GET /lab/projector/<id>.png` and `GET /lab/ui.png` capture the exact output and editor.
  `GET /lab/ui/tree` returns accessible widgets; `POST /lab/ui` injects a click, key, typing,
  drag or scroll event.
- `POST /lab/scan` scans every output; `{"projector":"left"}` scans one. Read results from
  `/lab/scan/<id>/photo.png`, `map.png`, `footprint.png`, and `/lab/found/<id>`.
- `POST /lab/realign` performs the drift check and re-alignment.
- `GET /lab/vision/<name>.png` returns available vision diagnostics such as edges, facets,
  shadow masks and tracked spots.

Every raw request needs `X-Lumap-Lab: <token>` and a localhost `Host` header. Actions mirror the
editor: open/demo/new/save, step/select, add a surface, set corners, apply a look, set a parameter,
store or GO a cue, identify, overlay or show a pattern, open/close outputs, go live/leave, undo,
redo, wait and choose a camera. Run the client with `--help` for its commands.
