# Upstream receive-loop stall fixes

This branch starts at the source of `nvpn-fips-core 0.4.115`, commit
`8ac53dbf9b69bfafdb1258ea69e6dfe23b5a6672`.

## Upstream sources

Johnathan Corgan's upstream FIPS v0.5.3 includes these relevant fixes:

- `76f81945109608c6bb13275b4092cadaca72568c`: stop dialing from handshake
  replies and other receive-loop sends (upstream issue #176).
- `27872ac863350b4e03716b7a565e33a0b9598c2b`: keep UDP hostname resolution
  off the receive loop.
- `5b53a78ed5ad2dc2c5fb02841d57d3b0659c2405`: resend lost link-rekey replies.
- `bf2510b0a5c064bb5eae9111e184b28c87797719`: exclude previous-session
  packets from new-session MMP state.

The nvpn fork has a different dataplane, endpoint API, transport ownership,
and rekey implementation. This is an adapted backport, not a wholesale merge
of upstream v0.5.3 or a claim that all its fixes are included.

## Integration contract

Node control sends use existing TCP/Tor connections, including completed
background connections, but never initiate a dial. Explicit outbound
connection preparation still starts background dials. UDP node sends use
numeric or cached addresses, never inline DNS. The fork already retains UDP
hostname resolution as a connection preparation before creating Noise state;
this avoids importing upstream's extra held-datagram and stale-address queues.
TCP background preparation also performs its DNS work inside its task.

The public transport send APIs retain connect-on-send behavior for callers
outside the node. Existing stream framing, write deadlines, cancellation
poisoning, connection generation ownership, and protected host sockets remain
in place. This change does not eliminate all receive-loop awaits.

Rekey recovery is already covered here by
`responder_rekey_msg1_resends_duplicates_and_replaces_orphaned_pending_state`.
Old-epoch link report and measurement isolation is covered by
`draining_link_epoch_reports_do_not_reach_current_handlers` and
`draining_link_epoch_packets_do_not_enter_current_receiver_counters`.
Those implementations are retained, not replaced with upstream's state machine.

No protocol encodings, message types, routing rules, crate versions, or public
API signatures change. No package release or nvpn installation is performed.
The November 2 endpoint-timeout incident has not been reproduced; these fixes
remove source-observed blocking paths, not a proven incident root cause.

## Verification

Linux verification used the pinned Rust 1.96.0 toolchain and locked dependencies.
The three new regressions failed on the original source and passed after the fix.
Final focused runs passed 63 TCP, DNS, Tor and maintenance tests plus five stream
boundary and link-epoch tests. The retained duplicate-rekey responder test passed
with `sim-transport` enabled. Endpoint tests, the all-features core compile check,
clippy with all core targets, the source-line check and `git diff --check` passed.
Clippy and compilation still report existing warnings.

The broader default-feature core run was not fully green: 2,794 passed, one failed
and 19 were ignored. `portable_send_preserves_datagram_boundaries_order_and_ready_batch_tail`
timed out waiting for a loopback datagram, including on an isolated retry. The
same isolated test failed on the unchanged baseline commit. Its raw-socket source
is not modified by this backport. Core formatting also reports an existing
mismatch in the unchanged `dataplane/tests/idle_control.rs` file; workspace
formatting additionally reports vendor differences. These are not passing gates.

The added send futures exposed a compiler query-depth limit in mutating control
dispatch. Boxing only the cold connect/disconnect command futures resolves it
without increasing the compiler limit or allocating per packet. Both independent
read-only reviews found no remaining actionable issues after the test fixes.
