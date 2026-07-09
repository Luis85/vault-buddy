## Summary

<!-- What changed and why. Link the design spec under docs/superpowers/specs/
     if one exists for this change. -->

-

## Test plan

<!-- What you actually ran, not what CI will run for you. Check the boxes
     that apply; delete the ones that don't (e.g. skip the Windows box for a
     docs-only or Linux-testable change). -->

- [ ] `npm test` (Vitest, frontend/store changes)
- [ ] `npm run build` (vue-tsc typecheck + production build)
- [ ] `cd src-tauri && cargo fmt --check`
- [ ] `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo test`
- [ ] Windows-only change (`src-tauri/src/*.rs`) — verified against
      `windows-app` CI, since it can't compile in this container
- [ ] Manually exercised the feature end-to-end (UI/UX changes)

## Related

<!-- Linked issue, PR, or docs/use-cases/*.md entry this touches. -->

-

---

CI gates: `frontend` (typecheck, tests, build) → `rust-core` (fmt, clippy,
tests) → `windows-app` (build + installer). Every PR also gets an automated
Codex review and GitGuardian secret scanning — treat their findings as real
leads and resolve the thread once addressed.
