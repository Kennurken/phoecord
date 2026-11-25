# Phase 2: ledger

Spec: `docs/phase-2/ai/spec.md` · Plan: `docs/phase-2/ai/plan.md`.

Line format: `YYYY-MM-DD HH:MM | <id> | claimed / paused / resumed / blocked / review / complete | <who> | <text>`, UTC, new lines at the bottom. Decisions: a `Ruling:` line, then `python scripts/gen_decisions.py`.

2026-11-25 23:00 | -     | plan-created | Nanda/Claude | Phase 2 plan pushed to main; Phoenix Canary available on the owner's account; next: 2.1.1
2026-11-25 22:03 | 2.2.5 | review | matvee | part 1 without the Phoenix binary: `phoecord_loader::locators` (Found<T>, LocatorError, exactly_one, winner, Image from .text/.rdata/.pdata, CallIndex of call rel32 and RIP-relative lea/mov), 5 tests on mock sections, branch feat/locators; written from the description in phoenix-desktop.md §5, no sunbird code; part 2 (ReqwestTransport, gateway registry, allocator shims) waits for 2.1.5/2.1.6
