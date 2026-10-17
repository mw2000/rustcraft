# rustcraft

A survival and exploration voxel game in Bevy, built on a renderer that draws the world out to the
horizon.

**[docs/design.md](docs/design.md) is the source of truth.** Read it before starting work: the
vision (§1), the invariants every change must keep (§3), and the roadmap of milestones (§6).
Update it in the same PR when your work finishes a milestone, settles a decision or changes the
plan.

- Build and run: `cargo run --release`. Test: `cargo test`. Keep `cargo fmt` and `cargo clippy` clean.
- Performance work: use the `profile` skill. Block textures: use the `block-textures` skill.
- Match the surrounding code's style: plain-prose comments that explain why, British spelling.
