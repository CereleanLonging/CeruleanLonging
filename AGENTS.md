# Active development workspace

## Source cache management and one authorized GitHub update (2026-11-04)

The user requests temporary compiler/build caches, automatic end-of-task cleanup with empty directories retained, and a cumulative cache monitor at the bottom of Maintenance. They explicitly limited the scope to this workbench source checkout, excluding user coding projects. Use the managed build/check/lint entry points described in `docs/BUILD-CACHE-MANAGEMENT.md`; distilled cache records remain local under `docs/build-cache/cleanup-logs`.

The user explicitly authorized one GitHub update after this feature is implemented and verified. This authorization covers the current update; future updates continue to require explicit authorization. Local historical distillation documents, cache logs and user media are excluded from source publication.

## GitHub publishing requires explicit authorization (user instruction, 2026-11-03)

The user now says: “后续不用每次更新都默认发GitHub，在我明确授权后再发”. Continue authorized local changes, testing and packaging, but do not automatically push source, update remote README/main, create or publish releases, or upload assets. Obtain explicit user authorization covering the intended GitHub update before performing it. This supersedes earlier standing authorization to publish every completed update. Do not request authorization again if the user has already explicitly authorized that specific update under this rule.

Version 0.1.43 was published before this instruction arrived. Leave that existing release intact. This policy update is local only until publication is explicitly authorized.

## Active workspace size target (user instruction, 2026-11-04)

The user requests a concrete storage inventory and optimization plan, with the workbench source and independent verifier within 10–20 GB. Apply a 20,000,000,000-byte acceptance ceiling to the complete active checkout, including its dependencies, Git data, build caches, local packages and reports. Account for historical sibling workspaces separately in the existing D:/test total.

Follow-up 791 removed 15,933 active debug-cache files totaling 28,753,516,057 logical bytes and measured the active checkout at about 8.73 GB. Its retained summary is `docs/build-cache/CLEANUP-791.md`. Follow-up 792 is the user's newer authorization to distill and remove all historical records and test environments, including an explicit reply authorizing the old Win10-358 disk. Historical knowledge and cleanup results are consolidated under `docs/build-cache/`; consult `HISTORY-SUMMARY.md` and the latest handoff for actual completion. Current source, Git, current usable package, original 0.1.34 package and user projects remain protected. Build-profile changes and automated 15/18/20 GB thresholds are still proposed, not implemented. Future storage-increasing work must reconcile the active-directory ceiling with the existing global capacity check.

This is the only active source checkout for the post-0.1.34 workspace/verification repair. Its initial tree is exactly `62553af3ec6ec89e8a4ff88d32710b8be379d948`, reconstructed from all 1,212 locally retained published blobs. Follow-up 765 fetched the real upstream commit `2ef805ddb47b1f53839985ba3675b14e8279f6a9`, verified the identical tree, and aligned the development branch without altering working files. The old reconstructed root is retained only on `archive/reconstructed-0134-764`; never publish or force-push that archive branch.

Read `docs/HANDOFF-LATEST.md` for current status and `docs/build-cache/HISTORY-SUMMARY.md` for distilled history. The user's 792 cleanup authorization supersedes the earlier requirement to keep historical source checkouts, frozen snapshots and raw evidence. Do not infer that a deleted historical path is an active instance or restart old commands from archived summaries. The original 0.1.34 package remains at its protected location under `../CeruleanLonging-0134-713/reports/`.

The current request authorizes historical distillation and cleanup. Do not use retired model credentials or create VMs. Role directories are runtime project directories, not separate copies of this Rust/TypeScript source. Chief requirements must be outside worker write scopes; deputy reads through the host's bounded, authenticated material path. Historical approvals and unresolved attempts are recorded in distilled documents, with failed and deferred outcomes kept distinct.

## Continuing operational rules

- Before storage-increasing work, run `scripts/check-test-storage.py` with the estimated allocation. The global D:/test ceiling is 120,000,000,000 bytes and at least 10GB physical free space must remain, alongside the stricter active checkout ceiling.
- Reuse verified mechanisms and test only relevant differences. Report native desktop, browser fixtures, simulated models and real provider results within their actual scope. Ignored or unexecuted tests are not passes.
- Preserve user applications and unsaved data. Use normal shutdown for owned services. A cancelled OS UAC must not be automatically reopened without the user's renewed timing instruction.
- Prefer PowerShell literal paths for Windows file operations. Verify absolute targets before recursive deletion or moving. Use extended paths for long-path metadata and hashing.
- PowerShell 5 JSON receipts should contain projected primitive fields, using ReadAllText for raw strings. Do not serialize attached provider/CIM objects. Wait for process exit and output EOF before final receipts.
- New frozen evidence should refer to immutable snapshots or distilled records, not assume mutable active source paths remain permanently unchanged. The 792 user instruction specifically replaces older raw-history retention requirements.


# Local release retention (user instruction, 2026-11-03)

Keep one original public 0.1.34 baseline at `D:\test\CeruleanLonging-0134-713\reports\p6-windows-package-0134-omp-713` and one current usable release. The user's 792 instruction authorizes removing superseded historical package copies and raw test records after distillation. Retain package identities and cleanup outcomes in `docs/build-cache/`. Remove new installer copies after explicitly authorized GitHub upload/hash verification as already requested. Current source, user data and GitHub releases remain outside historical cleanup. Check process ownership and use normal service shutdown before deleting an in-use package; no force termination of user applications.

The user's desktop entry is `C:\Users\姚\Desktop\CeruleanLonging 工作台.lnk`. When replacing the current package, verify this known shortcut still targets the old package, update its target, working directory and icon to the verified replacement, and read it back before removing the old archive. Never copy only the GUI executable to the desktop: the sibling runtime and packaged resources are required. The user-authorized standalone desktop GUI copy was removed in follow-up 780.
