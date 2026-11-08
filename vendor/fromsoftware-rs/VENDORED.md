# Vendored fromsoftware-rs

The `eldenring` and `fromsoftware-shared` crates of
[fromsoftware-rs](https://github.com/vswarte/eldenring-rs) (Vincent Swarte and
contributors, MIT OR Apache-2.0, see LICENSE-MIT and LICENSE-ASL2), as used by
Multiple Spirit Ashes / NativeTweaks:

- upstream snapshot 02fa568;
- plus the RVA bundles for Elden Ring WW 2.7.2.0, JP 2.7.2.1 and JP 2.7.1.1
  (`crates/eldenring/src/rva/`), which are not upstream.

Only the crates NPC Spawner builds are here; the workspace manifest lists just
those (the other games, tools, examples and tests were left out).
