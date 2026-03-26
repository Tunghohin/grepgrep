<p align="center">
  <img src="assets/logo_name.svg" width="220" alt="grepgrep logo" />
</p>

# grepgrep

`grepgrep` is a desktop audio practice and transcription app for musicians.

It is designed for learning songs by ear, slowing audio down without changing pitch, viewing the waveform, looping difficult sections, and navigating quickly while practicing or transcribing.

## Features

- waveform display
- loop region selection and repeat playback
- pitch-preserving speed control
- support for `mp3`, `flac`, `wav`, `ogg`, `aac`, and `m4a`

## WIP

- instrument audio separation is currently work in progress

## Build

Build for the current platform:

```bash
./build.sh current
```

Build targets:

```bash
./build.sh linux
./build.sh windows
./build.sh all
```

Run from source:

```bash
cargo run --release
```

Open a file at startup:

```bash
cargo run --release -- --file path/to/audio.wav
```
