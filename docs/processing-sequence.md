# Processing sequence

```mermaid
sequenceDiagram
    actor User
    participant UI as Slint UI
    participant App as Application layer
    participant Pool as Worker pool
    participant Probe as FFprobe service
    participant Encode as FFmpeg process

    User->>UI: Select input and output folders
    User->>UI: Scan
    UI->>App: scan callback
    App->>Probe: Probe discovered videos
    Probe-->>App: Metadata rows
    App-->>UI: Display queue

    User->>UI: Start
    UI->>App: start callback
    App->>Pool: Start N workers

    loop Until queue is empty
        Pool->>Probe: Probe next file
        Probe-->>Pool: Duration and resolution
        Pool->>Encode: Start FFmpeg with partial output
        Encode-->>Pool: Progress records
        Pool-->>App: Update queue state
        App-->>UI: Refresh progress
        Encode-->>Pool: Successful exit
        Pool->>Pool: Rename partial output
    end

    Pool-->>App: All workers complete
    App-->>UI: Ready
```

## Compression Advisor sequence

1. Read dimensions, duration, and frame rate with FFprobe.
2. Fit the source inside the maximum resolution without upscaling.
3. Convert target size into an audio-plus-video bitrate budget.
4. Compare the video budget with the selected codec's quality floor.
5. Display predicted size and quality before encoding.
6. Encode to a partial output file.
7. Measure the partial file.
8. If it is too large and retries are enabled, reduce bitrate and encode again.
9. Rename the accepted partial file into its final output name.
