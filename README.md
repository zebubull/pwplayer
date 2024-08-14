# pwplayer
pwplayer is a simple music player for pipewire written in rust.

## Usage
`cargo run -- <path-to-file>`
NOTE: currently only mp3 is supported

## Control
pwplayer exposes a unix-domain socket at `/tmp/pwplayer.sock` that can be used to control the player via `netcat -U /tmp/pwplayer.sock` or similar. It will not handle concurrent connections. The following commands are available:
- `play` will begin playback
- `pause` will pause playback
- `toggle` will toggle playback
- `volume [volume]` will set playback volume
- `done` will close the current connection
- `quit` will terminate the player

## Copyright
Copyright (c) 2024 zebubull. All Rights Reserved.
