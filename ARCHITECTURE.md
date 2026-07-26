# Architecture

## Design goals

Video Squeezer separates user-interface work from media-processing work so that the window stays responsive while several FFmpeg processes run concurrently.

The most important rule is:

> The GUI never runs FFmpeg directly, and worker threads never manipulate widgets directly.

## Layers

### Application layer — `src/app`

This layer owns the Slint window and translates user actions into application operations.

- `mod.rs`: creates the window and starts the event loop
- `callbacks.rs`: connects buttons, folder pickers, queue selection, start, pause, and stop
- `settings.rs`: validates controls and creates an immutable `JobConfig`
- `view.rs`: converts Rust state into Slint properties

### Model layer — `src/models`

This layer contains data only. It has no FFmpeg command construction and no GUI callbacks.

- `settings.rs`: validated processing configuration
- `state.rs`: synchronized shared application state
- `video.rs`: one queue item and its lifecycle state

### Scheduler layer — `src/scheduler`

This layer decides which files run and how many run at once.

- `pool.rs`: creates a bounded worker pool and hands out queue indexes
- `worker.rs`: processes one file from probe through final rename

### Service layer — `src/services`

This layer talks to external programs and the filesystem.

- `scanner.rs`: walks the input tree and creates queue entries
- `ffprobe.rs`: reads structured metadata
- `encoder.rs`: chooses software or VideoToolbox encoders
- `thumbnails.rs`: generates previews and contact sheets

### Utility layer — `src/utils`

Small reusable helpers that do not own application policy.

## Concurrency model

`SharedState` is stored in an `Arc`, allowing worker threads to share ownership safely. Mutable collections are protected with `Mutex`. Frequently read yes/no flags use atomics.

Each worker:

1. Claims the next queue index.
2. Reads a snapshot of that queue row.
3. Probes the video.
4. Starts one FFmpeg process.
5. Parses FFmpeg progress output.
6. Updates the queue row briefly under the mutex.
7. Requests a GUI refresh on Slint's event-loop thread.
8. Renames the completed partial file.

The selected concurrency value controls the number of worker threads and therefore the maximum number of simultaneous FFmpeg processes.

## Failure boundaries

- Preview failure does not stop encoding.
- An FFmpeg failure marks only that queue item as failed.
- A partial output is never promoted unless FFmpeg succeeds.
- Cancellation kills running FFmpeg children and removes partial outputs.
- The original video is never deleted or overwritten.

## Dependency direction

Higher layers may call lower layers:

```text
app -> scheduler -> services
 |         |           |
 +-------> models <-----+
             ^
             |
           utils
```

Services must not import the application layer. Models must not know about callbacks or external processes.
