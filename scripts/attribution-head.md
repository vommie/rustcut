# Attribution

Only what the licenses require when a built RustCut is passed on. `scripts/attribution.py` writes this file.

RustCut's own source code is under the MIT License, see `LICENSE`. The built programs as a whole are under the GPL-3.0-only, because
they link the libmpv and FFmpeg libraries of the system they are built on, which are usually built under the GPL (Arch and Manjaro do);
their source is at <https://github.com/vommie/rustcut>.
Long license texts stand once in `LICENSES/`.

## Named because the license asks for it

- **Slint** by SixtyFPS GmbH, used under the GPL-3.0-only (`LICENSES/GPL-3.0-only.txt`). Source: <https://github.com/slint-ui/slint>.
- **libmpv2** and **libmpv2-sys**, compiled in, under the LGPL-2.1 (`LICENSES/LGPL-2.1-only.txt`).
  Source: <https://github.com/kohsine/libmpv-rs>.
- **option-ext**, compiled in, under the MPL-2.0 (`LICENSES/MPL-2.0.txt`). Source: <https://github.com/soc/option-ext>.
- **Tabler Icons**, MIT, copyright line and text below.
