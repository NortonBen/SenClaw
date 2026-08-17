# MLX native path — resource benchmark

- Host: Mac16,8 (12 CPU cores)
- Model: `mlx-community/gemma-4-e2b-it-4bit`
- Sampling: every 0.5s, greedy decode, 2 timed turns/run
- GPU idle floor before the run: 0.0% busy, 532 MiB GPU-resident

GPU busy-percent is **whole-device and system-wide** — it is not per-process and not an occupancy measure. Read it against the idle floor above.

| variant | scenario | prompt→gen | decode tok/s | TTFT s | CPU % (mean) | CPU s | GPU % (mean/peak) | RSS peak MiB | MLX active/peak MiB | GPU mem peak MiB |
|---|---|---|---|---|---|---|---|---|---|---|
| noring | short | 500→300 | 56.5 | 1.03 | 46.5 | 7 | 70/100 | 2848 | 2622/4216 | 5949 |
| ring | short | 500→300 | 57.0 | 1.02 | 49.5 | 7 | 72/100 | 2919 | 2622/4216 | 6443 |
| noring | medium | 4000→800 | 54.9 | 0.40 | 40.7 | 6 | 79/100 | 2847 | 2928/4534 | 7605 |
| ring | medium | 4000→800 | 55.2 | 0.40 | 39.8 | 6 | 79/100 | 2925 | 2928/4534 | 7766 |
| noring | long | 12000→1500 | 51.4 | 0.45 | 21.8 | 6 | 91/100 | 2863 | 3168/4930 | 7483 |
| ring | long | 12000→1500 | 51.3 | 0.45 | 20.8 | 6 | 91/100 | 2939 | 3168/4930 | 7459 |
| noring | short | 500→300 | 56.8 | 1.03 | 49.6 | 7 | 79/100 | 2847 | 2622/4216 | 5384 |
| ring | short | 500→300 | 56.8 | 1.02 | 51.5 | 6 | 77/98 | 2920 | 2622/4216 | 5813 |
| noring | medium | 4000→800 | 54.8 | 0.40 | 40.6 | 6 | 82/100 | 2923 | 2928/4534 | 8072 |
| ring | medium | 4000→800 | 55.3 | 0.40 | 37.8 | 6 | 83/100 | 2922 | 2928/4534 | 7374 |
| noring | long | 12000→1500 | 51.4 | 0.45 | 19.4 | 6 | 90/100 | 2865 | 3168/4930 | 6999 |
| ring | long | 12000→1500 | 51.8 | 0.44 | 19.3 | 6 | 90/100 | 2938 | 3168/4930 | 7512 |

`CPU %` is derived from cumulative CPU-time deltas, so 100% = one core fully busy and the ceiling is 1200%. `CPU s` is total CPU seconds for the whole run, which is the figure to compare when wall time differs between variants.