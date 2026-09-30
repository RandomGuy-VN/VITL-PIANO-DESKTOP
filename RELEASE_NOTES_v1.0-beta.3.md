# 🎹 VITL Piano Desktop V1.0 Public Beta 3

The Piano Roll Studio rebuilt from the ground up into a MIDI editor you can actually compose in.

> **This release ships the Linux build only.** Windows stays on
> [Beta 2](https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/tag/v1.0-beta.2) until a Windows
> build of Beta 3 is attached here, which is why this release is marked as a pre-release — `releases/latest`
> keeps pointing at Beta 2 so `install.ps1` and the in-app updater carry on working for Windows users.

---

### 🐧 Install on Linux

```bash
curl -fsSL https://raw.githubusercontent.com/RandomGuy-VN/VITL-PIANO-DESKTOP/main/web/install.sh | bash
```

Or grab the package directly: [`vitl-piano-linux.zip`](https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/download/v1.0-beta.3/vitl-piano-linux.zip)

Then run `vitl-piano`, or launch **VITL Piano** from your application menu.

### 🪟 Windows

Stay on Beta 2 for now:

```powershell
irm https://raw.githubusercontent.com/RandomGuy-VN/VITL-PIANO-DESKTOP/main/web/install.ps1 | iex
```

---

## 🌟 What's New — Piano Roll Studio, rebuilt

### 🎛️ A roll that scales to any song
The grid, ruler, keybed and velocity lane are now viewport-sized canvases over a scroll spacer, with
binary-search viewport culling and a flat draw path for dense material.

- A 20 minute file keeps a **1120×509** canvas instead of asking the browser for **96,000px** — past the
  canvas limit the old roll simply painted nothing.
- **120,000 notes** scroll at roughly **21ms a frame**.

### 🎼 A grid that is actually musical
Bars, beats and subdivisions are derived from the song's tempo map, tempo changes included.

- Snap from **1/1 down to 1/32**, plus **triplets**, or free.
- Selectable **beats per bar** (2–7).
- The tempo field performs a true tempo change: notes keep their beat positions, so the roll never drifts
  out from under the grid.
- The old menu measured snap in milliseconds (`1/16 (125ms)`), which was only true at 120 BPM.

### ✏️ Real note editing
The Select tool used to do nothing at all — a note, once drawn, could not be moved, resized or deleted on
its own.

- Draw and drag out a length; move; resize from the right edge.
- Marquee select, `Alt`-drag to duplicate, sweep to erase, right-click to erase from any tool.
- **Undo / redo**, copy, cut, paste and duplicate.
- Selection-aware **quantize, legato, humanize, transpose and nudge** — applied to the whole song when
  nothing is selected.

### 🎚️ Velocity, audition and transport
- A **velocity lane** you sculpt by dragging across it.
- An **interactive keybed** that auditions pitches, and every note you draw or move sounds as you edit.
- **Transport in place**: play, pause, a scrubbable ruler and follow-playhead, so you never leave the
  editor to hear the edit. Unsaved edits are pushed to the player automatically before playback.

### ⌨️ Keyboard shortcuts
`1`/`2`/`3` tools · `Space` play · `Ctrl+Z` / `Ctrl+Shift+Z` undo & redo · `Ctrl+A/C/X/V/D` ·
`Del` · arrows to nudge and transpose · `Ctrl`+wheel zoom · `Shift`+wheel pan · `Ctrl+S` save.

---

## 🐞 Fixes

- **Saving a new composition silently timed out.** `Song` carries no serde defaults, so the missing
  `source_type` field made the backend drop the whole `save_song` frame — the editor then waited 2.5s and
  reported a timeout with no explanation.
- **Saved songs never reached the Library.** `SaveSong` wrote the `.mid` into a directory that only the Hub
  download and MuseScore import ever create, with the error discarded. On a fresh install the save reported
  success and wrote nothing. The directory is now created first, and the notification names the path it
  wrote — or says why the write failed while making clear the song still reached the player.
- **`Space` and `Escape` fired twice in the Studio**, hitting both the editor and the global transport
  hotkeys.

---

## ✅ Verified

End to end against the Rust backend, and again from a clean extraction of `vitl-piano-linux.zip`: a real
MIDI loads into the roll, edits mark the song dirty, Save round-trips and clears the flag, a brand-new
composition saves, the written `.mid` re-parses through the app's own parser with the exact notes drawn,
`/api/export_midi` returns a valid `MThd` file, and `Space` drives the real player.

`cargo test --release`: 46 passed.

---

### 🔐 Checksums (SHA-256)

| File | Size | SHA-256 |
| --- | --- | --- |
| `vitl-piano-linux.zip` | 39,736,047 | `0353ba6b1364977e97d996fab534fc988511bbbcef943d91cbe8aac78ad91c9b` |
| `vitl-piano-desktop` | 12,915,768 | `73d2a22f341093c83a11801eb32e13243c30412d54558a48c0a4be1f7df721be` |
| `desktop.html` | 264,770 | `42832a5096486bb79a65c933317c3f93076ac6a90c86e5a9b61ee6645cf50492` |
| `install.sh` | 12,367 | `f044dccde477ca08a5d33e92a8e0c11f33410d8876c0d8d277c345873e8caf52` |

**Full changelog:** https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/compare/v1.0-beta.2...v1.0-beta.3
