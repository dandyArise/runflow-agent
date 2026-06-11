# Changelog

## Unreleased

## 0.1.8

- Added `watch --once` and continuous `watch` mode for passive local RunFlow snapshots.
- Added `oncall` handoff generation for failed and unstable runs.
- Added `autopilot plan` dry-run proposals while keeping mutation apply paths unavailable.
- Added detailed post-V1 design docs for watch, oncall, and autopilot safety boundaries.

## 0.1.7

- Expanded `inspect-workspace --health` coverage for missing directories, invalid drafts, incomplete manifests, failed runs without failed steps, and orphan logs.
- Added `docs/v1-roadmap.md` for V1 completion status and post-V1 boundaries.
- Added `docs/post-v1-agent-modes.md` to open the watch, oncall, and autopilot design track without runtime behavior.

## 0.1.6

- Added `inspect-workspace --health` for local workspace integrity checks.
- Improved `doctor` with workspace integrity, audit, and deny-by-default permission checks.
- Added Windows release archive and install smoke tests.
- Added V1 status docs, release docs, and CI badge.

## 0.1.5

- Added `inspect-workspace` to inventory local jobs, drafts, runs, and follow-ups.
- Published Windows, Linux, and macOS release assets from tag `v0.1.5`.

## 0.1.4

- Added macOS ARM release asset support.

## 0.1.3 and earlier

- Built the assist-only V1 MVP with `doctor`, `draft`, `review`, `explain-run`, `report daily`, and `self update`.
