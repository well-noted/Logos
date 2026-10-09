# View window spike

Phase 0 throwaway code for criterion A4 (`docs/phase-0-spec.md`). It opens a model in the bridge, runs it with view frames streamed, and shows the frames in a plain window. When the run finishes, it prints how many frames were received and shown.

It uses `winit` and `softbuffer`, which give a window and a pixel buffer without choosing a UI framework for Logos. Nothing outside `spikes/` depends on this code.

## Run

Build the bridge jar first (`cd bridge` then `.\gradlew.bat jar`). Then, from this folder:

```
cargo run --release -- ..\..\corpus\own\aa4-combined\AA4-Combined.nlogox --setup "set run-seed 1 setup"
```

Options:

| Option | Default | Meaning |
|---|---|---|
| `--setup "<commands>"` | `setup` | Commands run once before the run starts |
| `--go "<commands>"` | `go` | Commands run repeatedly |
| `--fps <n>` | 30 | Maximum frames per second sent by the bridge |
| `--every <n>` | 1 | Send a frame at most every n ticks |
| `--scale <x>` | 1 | Window size relative to NetLogo's view |
| `--exit-when-done` | off | Close the window when the run finishes |

The window title shows the current tick and the frame rate shown. Closing the window stops the run and shuts the bridge down.

## What it measures

The summary line looks like:

```
run Stopped: 552 iterations in 13076 ms (42.2 iterations/s); frames received 377 (29.0/s), shown 376 (28.9/s) over 13.00 s
```

Record Windows results in `docs/phase-0-results.md` under A4.
