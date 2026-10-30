# Build notes: ramify mem

## Automated cases

Q1, Q2, Q4 to Q16 are `check` lines in the "mem reports and reclaims" section of `test/smoke.sh`. All pass.

## Cases run by hand

- **Q3**: the runewarden stack on this machine. `top` gives its group 538 MB. `ramify mem` gives 537 MB. RSS gives 26 MB.
- **Q17**: `ramify mem` on this machine listed 7 stacks. Each verdict matched `ramify dash` and the work orders. `oxc-toolchain` and `sbom-timeout` were at `finale`. The others were `ok`.

## Found during the build

- The first real run was on a branch without the fix from #28. The old clone `~/dev/rocko/gatekeeper` read the live state dir and called every live stack `orphan`. `--apply` would have stopped them. This branch now contains `fix/clone-state-collision`. Merge #28 first.
- The tag scan matched its own `awk` command line. It now requires a tag value, `project@hash/…`.
- A group whose only live member is `/bin/sleep` shows no tag: macOS hides the environment of platform binaries. The guard leaves it alone. `mem --apply` now says "left alone" instead of "stopped".
- A port that answers while no group of the stack lives belongs to another process. Such a stack is no longer listed.
- Fixture sleeps from a run that stopped half way made later runs fail. The teardown now kills them.

## Suite

`test/smoke.sh` all passed. `test/codex.py` `test_resume_preserves_unfinished_build` fails, as on `main`.
