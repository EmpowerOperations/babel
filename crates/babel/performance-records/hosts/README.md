# Hosts

What the `host` column in the ledgers means. A throughput number is only
comparable within a host, and the OS's idea of a machine's name is not always
a helpful one, so this is the index from the name a ledger row carries to the
machine that produced it.

| host | which | notes |
|---|---|---|
| `BATOU` | Geoff's desktop | Ryzen 7 7800X3D, Radeon RX 7800 XT. The reference machine for every row before 2026-09. |
| `DESKTOP-KH0S3BP` | Geoff's laptop | Ryzen 7 PRO 4750U, integrated Vega. Where the brute-squad work started. |

The writers take `BABEL_HOST` if it is set and `COMPUTERNAME` (or `HOSTNAME`)
otherwise, so a machine with a noise name can label its rows deliberately.

## `<host>.txt`, beside this file

The benchmarks describe the machine they ran on, in a dozen lines, every time
they run — `just bench` or `just brute` — via `common::describe_host` in the
test harness. It reads `sysinfo`, so it is the same lines on Windows, Linux and
macOS, with no elevation and nothing installed. It rewrites the file only when
the content changed and carries no timestamp, so a diff under `hosts/` means
the hardware or the toolchain moved, and nothing else.

Coarse on purpose. CPU-Z was tried and rejected: its report is mostly PCI
registers and SPD tables, and it needs a driver. A reader of a ledger wants to
know which machine and roughly what class of machine.

A GPU line will join when the GPU tier lands, from wgpu's adapter info.
