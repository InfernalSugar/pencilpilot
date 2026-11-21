# FFmpeg in PencilPilot

PencilPilot's installers include **FFmpeg**, a separate program the app runs to write
video exports (MP4, ProRes, WebM). It is installed next to the app as
`pencilpilot-ffmpeg`. PencilPilot sends it picture frames through a pipe; the two are
separate programs.

FFmpeg is free software, © the FFmpeg developers (https://ffmpeg.org). These builds are
licensed under the **GNU General Public License, version 3** (they include libx264,
libvpx and other GPL libraries). The licence text is in `GPL-3.0.txt` next to this file.
You may use, change and share FFmpeg under that licence.

## The builds and their sources

| Platform                         | Build                                                                                          | FFmpeg source                                    | How it was built                                                                    |
| -------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------ | ----------------------------------------------------------------------------------- |
| Linux (x86-64), Windows (x86-64) | BtbN FFmpeg-Builds, release `autobuild-2026-10-31-12-59`, `n9.0.2-39-g7adfbe40dc`, GPL variant | https://github.com/FFmpeg/FFmpeg/tree/7adfbe40dc | https://github.com/BtbN/FFmpeg-Builds (scripts and the versions of every library)   |
| macOS (Apple silicon)            | Martin Riedl's FFmpeg build server, release 9.0.2 (`1789931890_9.0.2`)                         | https://github.com/FFmpeg/FFmpeg/tree/n9.0.2     | https://ffmpeg.martin-riedl.de (build scripts: https://gitlab.com/martinr92/ffmpeg) |

The complete corresponding source of these builds is the FFmpeg source above plus the
sources of the libraries each build script names, at the versions it pins.
