# Open Larager

- Prefix shell commands with `rtk` (use `rtk proxy` for unfiltered tools).
- Target Windows 11 x64. Keep OS operations in Rust; React communicates through typed IPC.
- Ship no server binaries. Install only user-selected, SHA-256-pinned packages from the curated catalog.
- Keep binaries, configuration, databases, logs and projects separate. Never delete user data during package removal or silently migrate database versions.
- Bind managed services to loopback. Never expose arbitrary shell execution or unrestricted filesystem access through IPC.
- Keep all interface strings in the five complete locale dictionaries and respect system language and reduced motion.
- For backend changes run `cargo test`, `cargo clippy --all-targets -- -D warnings`; for UI changes run `pnpm build` and relevant Playwright checks.
- Keep documentation concise and accurate. Record unverified release requirements instead of claiming production certification.
