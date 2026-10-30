# lumap manual

This is the user guide. The README has the two-minute start; this page covers everything
else, in the order a show comes together.

## 1. Outputs and projectors

An **output** is one projector. Create them from the welcome screen (*Create N output(s) with a test
grid* makes one per secondary display, placed and full size) or with *+ output* in the tree.

- **Placement** ties an output to a monitor. *Fill monitor …* sets size and position from the
  OS; *Use current window position* takes the preview window where you dragged it. Placed
  outputs open borderless and always on top. Unplaced ones open as draggable previews.
- **Identify** (`I`) flashes every output's id, size and a colour on its projector for three
  seconds.
- **Edit on the wall**: while not in show mode, output windows draw the same handles as the
  canvas and accept the same drags and keys, so you can calibrate while looking at the
  surface. *View → Handles on output windows* turns it off. The cursor hides after two seconds
  of stillness on an output.
- **Stage view** (`T`) draws every placed output to scale on the virtual desktop with monitor
  outlines; overlaps are highlighted with their size in pixels. Click selects, drag moves the
  placement.
- **Colour**: brightness, contrast and gamma per output.

## 2. Surfaces and warps

A **surface** puts a source on an output. Add one with *+ surface* (an output must be
selected or exist).

- **Corner pin**: drag the four corners. Shift-drag moves the whole surface. Arrow keys nudge
  the active handle by one output pixel; Shift ×10, Alt ×0.1. The status bar shows the active
  handle in output pixels.
- **Bezier mesh**: *Convert to Bezier mesh* (choose columns and rows), then drag the control
  points. *Reset to flat mesh* and *Back to corner pin* are one click.
- **Canvas**: scroll to zoom, middle-drag or Ctrl-drag to pan, `0` fits. `G` toggles a grid in
  output space, `S` snapping to edges, centre and grid lines.
- **Align** menu: flip horizontal/vertical, rotate 90°, fit to output, centre, match the
  source's aspect.
- **Lock** (`L`) hides the handles so a finished surface cannot be nudged by accident.
  **Duplicate** (Ctrl+D), **Forward/Backward** change draw order, rename in the inspector,
  right-click a tree item or the canvas for the same actions.
- **Masks**: *Add mask* creates a feathered polygon (hole or window); drag its points on the
  canvas.
- **Opacity**, **blend mode** (normal, add, multiply, screen) and **crop** (drag the rectangle
  on the preview) are in the inspector.

## 3. Sources

*Add media…* opens a file dialog; *Add frame folder…* adds a numbered image sequence; or drop
files anywhere on the window.

| Kind | Notes |
|---|---|
| Test pattern | Grid, bars, checker, ramp. The grid has a crosshair and a circle to reveal aspect errors |
| Solid | Colour with alpha |
| Image | PNG, JPEG |
| Video | Any format `ffmpeg` decodes. The installer bundles ffmpeg; see "Where ffmpeg comes from" in section 7 |
| Sequence | Folder of numbered PNG/JPEG frames, or a GIF, with a frame rate |
| Generator | Twenty patterns in six groups, speed and tint: **Edges** (edge chase, outline, scan), **Facade** (bricks, windows, blocks, wireframe), **Nature** (water, fire, aurora, stars), **Party** (disco, kaleidoscope), **Romance** (hearts), **Classic** (plasma, noise, colour cycle, strobe, radar, grid pulse) |
| Spout | Windows builds: any Spout sender by name (empty = the active sender) |

**Looks** are ready-made generators with a colour, speed and blend mode: *Try a look* on the
welcome screen puts one on every surface; the surface inspector's *Looks* section puts one on
that surface (one undo step either way); `lumap new FILE --look NAME` writes the demo with it;
`lumap looks` lists them. Change the generator's tint or speed afterwards like any source.

Video, sequences and generators share a **transport**: play/pause, loop, speed, restart. They
are also parameters (`/source/<id>/play`, `/loop`, `/speed`, `/restart`) so cues, OSC and MIDI
drive them.

## 4. Blending and calibration overlays

Where two projectors overlap, each must fade out across the overlap so the light adds up to
one projector's worth.

1. Place both outputs (section 1) so their rectangles overlap on the desktop by the amount
   the projectors actually overlap on the wall.
2. Press **Suggest blends** in the show inspector. lumap sets left/right (or top/bottom) blend
   widths from the overlap.
3. Put the **Blend check** overlay on both outputs: flat white, so any band or seam is the
   blend's fault. Adjust the widths and curves until the seam is invisible.
4. **Gamma match**: press *Gamma match steps* on both outputs (grey steps overlay), then press
   `[` or `]` on the output shown in the canvas until the steps match across the seam. The
   blend gamma is the projector's gamma; 2.2 is typical.

Overlays are per output (grid, crosshair, colour bars, grey steps, blend check), saved in the
project, and never shown in show mode.

## 5. Cues and show control

A **cue** is a snapshot of every live parameter (opacities, visibility, master, blackout,
output levels, blend widths, transport) plus a fade time.

- **Store this look** in step 4, Show, keeps the live look as a new cue after the selected
  one, with the name and fade typed above it. Cues are cards: click one to select it, then
  rename it (its id, used by OSC, stays), change its fade, **Update cue** with the live look,
  or delete it. Drag a card by its six dots to reorder; right-click for GO, update, move and
  delete.
- **GO** (`Space`, the big GO, a cue's own GO button, `/cue/go`, `/cue/<id>/go`, a MIDI
  button, the phone) fades to the next cue: numbers interpolate, booleans switch half way.
  Under GO, *Next* names the cue it will fade to; the running cue carries a *live* pill.
  **Stop fade** freezes a fade where it is.
- **Blackout** (`B`) and **master** are always live and are captured by cues too.

### OSC

`--osc-port 9000` (default; `0` disables), or the OSC card in step 4, Show: set the port and
*Listen* (lumap remembers it), *Stop OSC* to close it. The card's *OSC addresses* list
every address. One message per parameter, the first argument is the value; a message with no
arguments means `true`.

```text
/show/blackout bool           /show/master 0..1
/surface/<id>/opacity 0..1    /surface/<id>/visible bool      /surface/<id>/z int
/surface/<id>/corner/<0-3>/x|y 0..1 (quads)
/output/<id>/enabled bool     /output/<id>/brightness|contrast|gamma
/output/<id>/blend/<left|right|top|bottom>/width 0..0.5    /output/<id>/blend/gamma
/source/<id>/play bool        /source/<id>/loop bool    /source/<id>/speed    /source/<id>/restart
/cue/go                       /cue/<id>/go              /cue/stop
```

**OSC feedback**: type `host:port` into *Feedback host:port* on the OSC card and *Apply*.
Every change (from the UI, cues, MIDI or the remote) is sent back as the same address and
value, so a TouchOSC or Lemur layout follows the app.

### MIDI learn

1. The MIDI card in step 4, Show → *Refresh MIDI ports*, pick one, *Open MIDI port* (saved in
   settings).
2. Type the address to drive (or press *MIDI learn opacity* in a surface inspector, which
   fills it in) and choose **Learn fader** (0–127 scaled to the parameter), **Learn button**
   (sends `true` on press, for GO and restart) or **Learn switch** (`true` on press, `false` on
   release, for blackout and visibility).
3. Move the control. The mapping is saved in the project; *Forget* removes it.

### Phone remote

*Start remote* on the Phone remote card in step 4, Show (or `lumap show … --remote-port 8080`)
serves a page on
the LAN; scan the QR code with a phone. The page has blackout, master, output levels,
surface opacities and the cue list with GO. It is plain HTML and JavaScript, no app, no
internet. The JSON API behind it: `GET /api/state`, `POST /api/param {address, value}`,
`POST /api/cue/go {id?}`.

## 6. Show mode

**Go live** in the top bar checks the show first: projectors on their displays, content on
every surface, ffmpeg when there is video, the show saved (autosave and crash recovery need a
file) and cues to fade between. Each line ticks or offers one click that puts it right, such
as *Place on free displays* or *Store the first cue*; *Go live anyway* is always there. `F10`
goes live without the check, and `lumap show project.lumap.toml` starts in it. Editing is locked,
every enabled output opens where it is placed, OSC/MIDI/remote stay live, overlays are
hidden, autosave pauses, the editor window shows status only. Escape twice leaves.

`--restart-on-stall 5` relaunches the process if the render loop stops beating for five
seconds. Autosaves go next to the project; on the next open lumap offers to recover one.

## 7. Settings and files

Preferences live in `settings.toml` in the OS config directory (`%APPDATA%\lumap` on
Windows; `LUMAP_CONFIG_DIR` overrides): recent files, OSC port, MIDI port, OSC feedback
target, remote port, grid, snap, edit-on-wall, autosave interval, ffmpeg path and whether to
skip the welcome screen.

### Where ffmpeg comes from

Video needs `ffmpeg` and `ffprobe`. lumap looks, in order, at the ffmpeg path in the settings
(the `LUMAP_FFMPEG` environment variable; `LUMAP_FFPROBE` names ffprobe separately), then at
`ffmpeg\ffmpeg.exe` or `ffmpeg.exe` beside its own executable (the copy the Windows installer
ships), then at the PATH. The show inspector says which one is in use. The bundled copy is an
LGPL build; its licence and origin are in `ffmpeg\LICENSE.txt` and `ffmpeg\VERSION.txt` next
to it.

### Updates

Once a day, and never while show mode is on or an output window is open, lumap asks GitHub
whether a newer release exists. The show inspector's *Updates* setting decides what happens
next: *tell me* (the default) shows a line in the status bar with Download, Later and Skip;
*download, then tell me* fetches the installer first and offers *Install and restart* once it
has been checked against the release's SHA-256; *never check* turns it off. *Check now* asks
at once. Installing always asks: lumap saves, closes, the installer replaces it and opens it
again. Later keeps the offer quiet for a day; Skip never offers that version again. Developer
builds (any version that is not `x.y.z`) do not update. Downloads live in
`%LOCALAPPDATA%\lumap\updates`.

Projects are TOML with a `schema_version`; older files are migrated on load and
`lumap migrate` rewrites them. Media paths are stored relative to the project file when
possible. `lumap validate` reports every broken reference or degenerate warp.

## 9. The camera

A webcam that sees the whole wall lets lumap scan the room, find surfaces and react to people.
A laptop's own camera sitting on the projector works; a USB webcam on a tripod sees more of the
room's shape. On Windows, allow desktop apps to use the camera (Settings ▸ Privacy ▸ Camera).
The camera only runs while something uses it, and pictures never leave the computer.

**Scan the room** (step 1, Setup ▸ Camera). Pick the camera, *Look through it* to aim it so
every projector's picture is inside the frame, then *Scan the room*. Each projector in turn
shows white, black and stripe patterns while the others stay black: about twenty seconds each.
Keep the room dark and still. The result per projector is kept in `<show>.scan/<projector>/`
(`photo.png`, `camera.png`, `white.png`, `black.png`, `cam2proj.png`, `scan.toml`) and the
projector remembers its folder (`scan = "…"` in the project file). The scan sheet shows how much
of each projector the camera saw, offers *Use measured blends* and *See them in Map*.
*Check alignment* runs a two-second check per projector.

**Map with the wall** (step 2). *Wall photo* draws what the projector sees over the canvas
(slider for strength). With snapping on (S), corners snap to real corners and edges.
*Find surfaces* shows what the scan found as dashed outlines: click one, then *Use this surface*
or *Use all*. *Pick an object*: click anything on the photo to outline it, then *Make a surface*
or *Cut it out of* the selected surface. *Hands*: with Edit on the wall on, pinch near a corner
on the wall to grab it.

**Room content** (step 3, Library ▸ Room). Trace the room, Letters, Shadow fill, Shadow
monsters, Snow that settles, Bouncing light, Marble run; Paint with light (a phone torch, laser
or glow stick, in a dark room), Paint with your hand (pinch to draw, open hand to stop, fist to
clear) and the Camera itself. The content panel sets effect, tint and speed, or brush, colour,
width and fade; *Clear the paint* (or `/paint/<id>/clear`).

**Zones and gestures** (step 4, Show ▸ Camera). Drag across the camera picture to draw a zone;
its meter shows motion or presence. Type an address in the MIDI card's *Learn address* (for
example `/cue/go`), press *Learn as button* (or fader, or switch) and move in the zone or make a
gesture (pinch, open hand, fist, swipe left or right, thumbs up, point). Forget a mapping from
the list.

**Before going live** the check adds a camera line: *Check with the camera* tests that each
projector is still where it was scanned; *Re-align* moves every surface of a bumped projector to
follow it; if the camera itself moved, *Scan again*.

## 10. The lab

`lumap --lab` (port 7414, or `--lab-port`) serves a local automation API for driving lumap from
scripts on real hardware: change the show, put patterns on projectors, take camera photos and
screenshots, click through the interface by label, run scans. It listens on 127.0.0.1 only and
needs the token in `%LOCALAPPDATA%\lumap\lab\session.json`. See [lab.md](lab.md).

## 8. Keyboard

| Key | Action |
|---|---|
| Drag handle / Shift+drag | Move a point / the whole surface or mask |
| Arrows (+Shift, +Alt) | Nudge 1 px (×10, ×0.1) |
| Scroll, middle-drag, `0` | Zoom, pan, fit |
| `G` / `S` | Grid / snapping |
| `L`, Ctrl+D, Delete | Lock, duplicate, delete |
| Ctrl+Z / Ctrl+Y | Undo / redo (a drag is one step) |
| Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S | New / open / save / save as |
| `I` | Identify outputs |
| `T` | Stage view |
| `[` / `]` | Blend gamma of the shown output |
| `B` | Blackout |
| `Space` | GO |
| `F10`, Escape Escape | Go live without the check; leave show mode |
| `?` | This list, in the app |
