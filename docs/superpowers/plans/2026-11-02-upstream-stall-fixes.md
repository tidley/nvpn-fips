# Compatible upstream stall fixes

**Goal:** Integrate upstream receive-loop stall fixes into the exact
`nvpn-fips-core 0.4.115` source without replacing the compatible dataplane.

**Architecture:** Backport the non-dialing-send contract from upstream
`76f81945109608c6bb13275b4092cadaca72568c` and the off-loop DNS principle
from `27872ac863350b4e03716b7a565e33a0b9598c2b`. Keep existing public
connect-on-send APIs for external callers; node control sends use existing
connections and cached/numeric UDP addresses only. Initial UDP hostname
resolution already has a retained preparation owner in this fork. Move TCP
background-connect DNS into its task as well. Do not import upstream's
separate stale-DNS replay queue or replace this fork's rekey state machine.

**Tech stack:** Rust, Tokio, existing TCP/Tor pools and UDP cache.

## Execution

- [x] Add node regressions: a handshake arriving on a missing TCP connection
  must not dial its sender, while healthy TCP handshakes still complete.
  Add transport regressions for no-connection, completed background connects,
  and uncached hostnames. Run against the old source and retain failures.
- [x] Add non-dialing TCP/Tor send paths; preserve validation, write deadlines,
  cancellation safety, statistics, and connection-generation ownership.
- [x] Route node sends through the non-dialing contract. UDP sends must not
  invoke a resolver; explicit outbound preparation remains responsible for DNS.
- [x] Move TCP background-connect resolution into the spawned task, preserving
  host-network protection and connect cancellation.
- [x] Check existing rekey duplicate-reply and old-epoch metric tests against
  upstream fixes `5b53a78ed` and `bf2510b0a`; do not duplicate working recovery.
- [x] Run locked focused/core/endpoint checks, formatting, clippy and source
  line checks. Obtain two independent read-only reviews and fix findings.
- [ ] Publish reviewed source in the separate GitHub source fork; do not
  change the existing fork's branches, publish packages, or change nvpn.
  Mirror the reviewed branch if a supported approved destination exists.

## Compatibility and limits

No message types, byte values, wire formats, public signatures or routing
semantics change. Stream writes retain their existing bounded/cancellation-safe
behavior; this is not a claim that every receive-loop await has disappeared.
These fixes are not a demonstrated explanation of the November 2 incident.
The snapshot FIFO scheduling hypothesis remains a separate investigation.
