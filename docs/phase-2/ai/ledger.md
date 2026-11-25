# Phase 2: ledger

Spec: `docs/phase-2/ai/spec.md` · Plan: `docs/phase-2/ai/plan.md`.

Line format: `YYYY-MM-DD HH:MM | <id> | claimed / paused / resumed / blocked / review / complete | <who> | <text>`, UTC, new lines at the bottom. Decisions: a `Ruling:` line, then `python scripts/gen_decisions.py`.

2026-11-25 23:00 | -     | plan-created | Nanda/Claude | Phase 2 plan pushed to main; Phoenix Canary available on the owner's account; next: 2.1.1
2026-11-25 21:57 | 2.3.1 | review | matvee | part 1 without the Phoenix binary: `phoecord_loader::theme` (theme.toml → Hsla, hex/rgb/hsl colours, bad entries reported, ThemeWatcher 2 s poll), 5 tests, branch feat/theme-engine; part 2 (hook on the active theme global, Phoenix field names, default theme) waits for 2.1.5/2.1.9
2026-11-25 22:03 | 2.2.5 | review | matvee | part 1 without the Phoenix binary: `phoecord_loader::locators` (Found<T>, LocatorError, exactly_one, winner, Image from .text/.rdata/.pdata, CallIndex of call rel32 and RIP-relative lea/mov), 5 tests on mock sections, branch feat/locators; written from the description in phoenix-desktop.md §5, no sunbird code; part 2 (ReqwestTransport, gateway registry, allocator shims) waits for 2.1.5/2.1.6
2026-11-25 23:30 | 2.2.1 | reassigned | Nanda/Claude | 2.2.1 moved from matvee to Nanda
Ruling: task 2.2.1 (Ghidra / IDA project) is reassigned from matvee to Nanda (owner Nanda, 2026-11-25, "move 2.2.1 to Nanda" (translated)) — why: the project feeds the hook targets of 2.1.5–2.1.6 and 2.2.2, which Nanda owns — risk: Nanda's RE load grows; 2.2.5 (matvee) depends on the project
2026-11-25 23:45 | 2.1.2 | complete | Nanda/Claude | phoecord dev inspect-pe (read-only, object crate) built; Binary inspection section in docs/research/phoenix-recon.md with findings on local Phoenix install (0.1.0.2208): version.dll/winhttp.dll/winmm.dll/dbghelp.dll/dwmapi.dll imported, no wininet.dll, no delay-loaded DLLs, DependentLoadFlags=0, no manifest <dllDirectory>, GUARD_CF on, install layout and update scheme (flat app\, per-module modules/<name>-<ver>, separate DiscordSystemHelperPhoenix service); open question carried to 2.1.5-2.1.7: does the observed SetDefaultDllDirectories call actually restrict search before version.dll is probed (needs an empirical sideload test)
