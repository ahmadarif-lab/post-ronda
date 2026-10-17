# Post Ronda desktop benchmark

Measured on October 17, 2026 using the installed macOS desktop applications. This is a baseline for launch and idle resource use, plus a comparison of everyday HTTP workflows. It does not measure API server performance or sustained request workloads.

## Test machine

- MacBook Air, model `Mac16,12`, Apple M4, 10 CPU cores (4 performance + 6 efficiency).
- 16 GB unified memory; macOS 27.0.1, build `26A434`.
- All three installed executables are native arm64 builds.
- Post Ronda 0.2.1, Bruno 4.2.1, Postman 12.32.4.

## Results

The raw measurements are in [macos-air-m4-2026-10-17.json](benchmarks/macos-air-m4-2026-10-17.json). All nine launches completed, with 20 idle samples each. Installed executable hashes were checked again afterwards and were unchanged.

| Client | First window (s) | Idle RSS (MiB) | Idle CPU (% of one core) | Bundle (MiB) |
| --- | ---: | ---: | ---: | ---: |
| Post Ronda 0.2.1 | 0.389 | 85.8 | 0.00 | 6.62 |
| Bruno 4.2.1 | 2.221 | 394.4 | 2.16 | 499.39 |
| Postman 12.32.4 | 0.957 | 461.0 | 10.25 | 462.81 |

First-window values are the median of three launches. RSS is the median of the three per-run medians; CPU is the median of the three per-run sample means. A CPU reading of 0.00 means no CPU-time increase was resolved by `ps` during those idle samples, not a guarantee of zero CPU consumption.

| Client | Per-run median RSS range (MiB) | First-window range (s) | Processes during sampling |
| --- | ---: | ---: | ---: |
| Post Ronda | 82.3–92.0 | 0.383–0.393 | 1 |
| Bruno | 308.1–446.5 | 2.211–2.720 | 4 |
| Postman | 329.6–503.6 | 0.841–0.995 | 5 |

Post Ronda had the lowest sampled idle RSS, smallest installed bundle and earliest first visible window in this test. Bruno and Postman memory readings varied substantially between runs. This was a normal development laptop; other system activity and memory pressure were not controlled. Use these figures as an indicative local baseline, not a universal speed or RAM-saving guarantee.

## What this measures

Each client starts in a new process with a disposable profile. The script waits for its first visible window, allows 15 seconds to settle, then samples every second for 20 samples. Three rounds rotate the application order; clients run sequentially. Personal collections, settings, and sign-in sessions are not used.

- **Launch to first window:** elapsed wall time until CoreGraphics reports a visible, normal-layer window owned by the main process, at least 300 × 200 points. This can be a welcome screen or splash; it does not prove the editor is interactive. Polling adds up to approximately 50 ms, plus query overhead. These are fresh-profile launches with existing OS disk caches, not cold-boot measurements.
- **Idle RSS:** sum of resident memory reported by `ps` for the main process, its descendants, and any helpers whose executable lives in the application bundle. Shared pages can be counted more than once; this is not macOS physical footprint or Activity Monitor's Memory column. GPU allocations, compressed memory, and swap are not measured.
- **Idle CPU:** change in cumulative process CPU time over each actual sample interval, summed across the same process set. 100% represents one fully busy CPU core. Short-lived helpers between samples can be missed.
- **Installed bundle:** allocated disk space from `du -sk`, converted to MiB. This excludes user data, OS frameworks, and external tools such as system `curl` and Git. APFS allocation/compression can affect this number. It is not download size.

The first-run screens, window dimensions, and enabled background services can differ. Post Ronda's isolated-state mode disables its startup update check; Electron clients retain their fresh-profile defaults. No requests, collection imports, or account sign-ins are performed. Network access is not blocked. Treat the results as a fresh-profile baseline, not an equal-workload benchmark of mature projects. Window dimensions, executable SHA-256 hashes, versions, per-run measurements, and individual process samples are recorded in the raw data.

## Repeat the measurement

On macOS with Python 3.9 or newer, close the three clients and keep the desktop unlocked. Avoid builds, renders, downloads, and other heavy workloads while sampling.

```sh
python3 scripts/benchmark-desktop.py --output target/benchmark-desktop.json
```

The script refuses to measure an already running client. It launches installed executables directly, uses temporary profiles, and terminates only the process groups it creates. Electron clients receive `--user-data-dir`; Post Ronda receives `POST_RONDA_STATE_DIR` and a temporary project folder. It does not change the installed applications or require screen recording or Accessibility permissions.

To override application locations or timing:

```sh
python3 scripts/benchmark-desktop.py \
  --app '/Applications/Post Ronda.app' \
  --app '/Applications/Bruno.app' \
  --app '/Applications/Postman.app' \
  --runs 3 --warmup 15 --samples 20 --interval 1 \
  --output target/benchmark-desktop.json
```

## Everyday usage comparison

Post Ronda features below were checked against the local source at `bce880e` (0.2.1). Competitor features are based on official documentation checked on October 17, 2026; editions and later versions can differ.

| Workflow | Post Ronda | Bruno | Postman |
| --- | --- | --- | --- |
| Desktop rendering | Rust + egui; no browser runtime | Installed bundle includes Electron and its helpers | Installed bundle includes Electron and its helpers |
| HTTP without an account | Requests, saved projects, variables and environments | Local collections; no account needed | Lightweight client supports requests and history; workspace features require sign-in |
| Files and Git | `collection.json` + `environments.json`; initialize, origin, pull, commit and push in the app | Plain-text collections versioned with Git; integrated Git features depend on edition | Native Git supports local project files and cloud synchronization |
| Move an HTTP collection from Postman | Import v2/v2.1 and environments; export v2.1 | Postman import supported | Own collection format; import/export supported |
| Scripted tests and CI | Not included | Scripts, assertions, collection runner and CLI | Scripts, collection runner and CLI |
| Beyond ordinary HTTP | No dedicated protocol tooling | GraphQL, gRPC and WebSocket tooling | GraphQL, gRPC, WebSocket and other protocol tooling |

Sources: [Bruno products](https://www.usebruno.com/product), [Bruno getting started](https://blog.usebruno.com/bruno-tutorial), [Bruno editions](https://www.usebruno.com/pricing), [Bruno CLI](https://www.usebruno.com/product/cli), [Postman lightweight client](https://learning.postman.com/docs/getting-started/basics/using-api-client/), [Postman Native Git](https://learning.postman.com/latest-v-12/docs/use/native-git/sync), [Postman overview](https://learning.postman.com/docs/use/overview), and [Postman installation and CLI](https://learning.postman.com/docs/getting-started/installation/install-app/).

Post Ronda's intended advantage is a small native desktop client for manual HTTP debugging, saved local projects and familiar Postman collection exchange. Offline work and Git-friendly files are also strengths of Bruno. Postman also supports local workflows; calling it cloud-only would be inaccurate. Teams needing scripted regression suites, CI runners, advanced auth or dedicated protocol tooling should account for Post Ronda's current limitations. Imported scripts are skipped with a warning, and unsupported auth can require manual changes.

## Further validation

Before making claims about low-spec hardware or active workloads, repeat on an actual low-memory machine and use the same imported HTTP collection, window size, settings and response payloads. Useful next scenarios are a small JSON response, a 1 MiB JSON response, repeated requests, and navigating 1,000 saved requests. Record editor-ready time separately from first-window time. The absence of Chromium is an architectural advantage for baseline overhead; it does not by itself prove every operation is faster.
