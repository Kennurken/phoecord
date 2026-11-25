Checked against code: not applicable (no PhoeCord code touches Phoenix yet); against a local install of Discord Phoenix, file version `0.1.0.2208`, read on 2026-11-25; §2.1.5–§2.1.7 added 2026-11-26 from a local Ghidra dump (`local_dumps/transport_dump.txt`, gitignored, not committed).

# Phoenix recon (task 2.1)

Facts about a local install of Desktop Phoenix (`%LOCALAPPDATA%\DiscordPhoenix`), gathered with read-only tools: `phoecord dev inspect-pe`, Windows Explorer/PowerShell, `Get-AuthenticodeSignature`, and the resource APIs used only to read (never run) the embedded manifest. No sunbird code or binaries are reproduced here (see [`phoenix-desktop.md`](phoenix-desktop.md) §0 for the ground rules this note also follows); everything below is a fact about the shipped binary, not borrowed source.

## Binary inspection (2.1.2)

**Target:** `C:\Users\<user>\AppData\Local\DiscordPhoenix\app\Discord.exe`, 518,388,664 bytes, `FileVersion`/`ProductVersion` `0.1.0.2208`, `ProductName` "Discord Phoenix", signed by Discord Inc. (valid Authenticode signature; so are every other `.exe`/`.dll` directly under `app\`).

**PE headers.** PE32+ (x86-64), Windows GUI subsystem, entry point RVA `0x12ff7f60`, image base `0x140000000`. No .NET (CLR) metadata — confirms a native (Rust) client, not Electron/.NET. `DllCharacteristics = 0xc160`: `HIGH_ENTROPY_VA | DYNAMIC_BASE | NX_COMPAT | GUARD_CF | TERMINAL_SERVER_AWARE`. `GUARD_CF` means Control Flow Guard is on — an inline detour must still produce valid CFG-reachable targets or the hooked thread's indirect calls through hooked pointers can crash under CFG bitmap enforcement; this needs to be verified in 2.1.5/2.1.6 when the actual detour is written.

**Import table — 50 DLLs, 0 delay-loaded.** Of the sideload candidates from 2.1.2's checklist, five are imported directly (not delay-loaded):

| DLL | Functions imported |
|:---|:---|
| `version.dll` | 3 |
| `winhttp.dll` | 5 |
| `winmm.dll` | 2 |
| `dbghelp.dll` | 1 |
| `dwmapi.dll` | 5 |

`wininet.dll` is **not** imported (Phoenix's HTTP stack does not go through WinINet — consistent with an in-process Rust HTTP client rather than the system one). The rest of the import table is the expected native-Windows set: `kernel32.dll` (326 functions — the bulk of the surface), `ws2_32.dll`, `user32.dll`, `advapi32.dll`, `crypt32.dll`/`wintrust.dll` (certificate and signature checks), `d3d11.dll`/`d3d12.dll`/`dxgi.dll`/`dwrite.dll` (GPUI's rendering backend), `webauthn.dll`, several `api-ms-win-core-*.dll` forwarders, and `icuuc.dll` (ICU, text layout/i18n). No `wininet.dll`, no .NET, no sunbird-specific name was found; this matches a self-contained Rust binary.

**DLL search restrictions.** `kernel32.dll` imports include `LoadLibraryA/W`, `LoadLibraryExA/W` **and `SetDefaultDllDirectories`** — so the process does call `SetDefaultDllDirectories` at some point (the import alone doesn't say with which flags or how early; that needs a disassembly-level check at the call site, which this read-only pass does not do). The Load Configuration directory's `DependentLoadFlags` is **0** — the image itself requests no `LOAD_LIBRARY_SEARCH_*` restriction at the PE level. The embedded manifest (resource `RT_MANIFEST`, id 1; id 2 absent) carries `requestedExecutionLevel asInvoker`, DPI/heap settings, and one `<dependency>` on `Microsoft.Windows.Common-Controls` 6.0.0.0 — **no `<dllDirectory>` element**. So: nothing at the PE-header or manifest level restricts `Discord.exe`'s DLL search path; any restriction, if present, is applied at run time through the `SetDefaultDllDirectories` call kernel32 exposes being imported. Whether that call actually narrows the search before `version.dll` would be probed is an open question for 2.1.5 (needs either a debugger breakpoint on `SetDefaultDllDirectories` or an empirical test: drop a stub `version.dll` next to `Discord.exe` and see if it loads).

**TLS and sections.** One TLS directory with 4 callbacks (RVAs `0x12658790`, `0x12a74660`, `0x12abf1d0`, `0x12a74510` — typical of Rust's TLS-based thread-local destructors/initializers, not evidence of anti-tamper). 15 sections; besides the usual `.text`/`.rdata`/`.data`/`.pdata`/`.reloc`, there are Rust/LLVM-specific sections `.eh_fram`(e), `.fptable`, `.itt_not`(ify, Intel VTune instrumentation — a debug/profiling leftover, not stripped from this build), `.xdata`, and two Intel-IPP-library sections `IPPCODE`/`IPPDATA` (`r-x`/`rw-` — a statically linked codec or crypto library using Intel's Integrated Performance Primitives). Full layout (virtual address, virtual size, raw offset, raw size):

| Section | Virtual address | Virtual size | Raw offset | Raw size | Flags |
|:---|:---|:---|:---|:---|:---|
| `.text` | `0x1000` | `0x132c7376` | `0x600` | `0x132c7400` | r-x |
| `.rdata` | `0x132c9000` | `0x4b988c8` | `0x132c7a00` | `0x4b98a00` | r-- |
| `.data` | `0x17e62000` | `0x6a9ac8` | `0x17e60400` | `0x5d9a00` | rw- |
| `.pdata` | `0x1850c000` | `0x1733274` | `0x18439e00` | `0x1733400` | r-- |
| `.eh_frame` | `0x19c40000` | `0x58` | `0x19b6d200` | `0x200` | r-- |
| `.fptable` | `0x19c41000` | `0x100` | `0x19b6d400` | `0x200` | rw- |
| `.itt_notify` | `0x19c42000` | `0x149c` | `0x19b6d600` | `0x1600` | r-- |
| `.pdata` (2nd) | `0x19c44000` | `0x837c` | `0x19b6ec00` | `0x8400` | r-- |
| `.tls` | `0x19c4d000` | `0x43b9` | `0x19b77000` | `0x4400` | rw- |
| `.xdata` | `0x19c52000` | `0x50f3e6c` | `0x19b7b400` | `0x50f4000` | r-- |
| `IPPCODE` | `0x1ed46000` | `0x6b495` | `0x1ec6f400` | `0x6b600` | r-x |
| `IPPDATA` | `0x1edb2000` | `0x700` | `0x1ecdaa00` | `0x800` | rw- |
| `_RDATA` | `0x1edb3000` | `0xe04` | `0x1ecdb200` | `0x1000` | r-- |
| `.rsrc` | `0x1edb4000` | `0xf800` | `0x1ecdc200` | `0xf800` | r-- |
| `.reloc` | `0x1edc4000` | `0x171648` | `0x1eceba00` | `0x171800` | r-- |

This table is the base for AOB scans in 2.1.5/2.1.6/2.1.7: `.text` for code patterns, `.rdata`/`.xdata` for vtables and unwind data (the HTTP-transport vtable slots 3.1 describes would be here), `IPPCODE`/`IPPDATA` for the statically linked IPP routines (unrelated to hooking targets, noted so they aren't mistaken for something else).

**Install layout** (`%LOCALAPPDATA%\DiscordPhoenix`):

```
DiscordPhoenix/
  app/                          current build, unversioned folder (no per-version subfolder like Electron Discord's app-<ver>)
    Discord.exe                 main binary (518 MB)
    "Discord Phoenix Crash Handler.exe"
    DiscordHook.dll, DiscordHook64.dll          (named like Electron Discord's low-level keyboard/mouse hook DLLs)
    DiscordHookHelper.exe, DiscordHookHelper64.exe
    DiscordPhoenixSetup.exe     installer/updater binary, also the uninstall entry point
    DiscordSystemHelper.exe     registered as the Windows service "DiscordSystemHelperPhoenix" (stopped by default)
    discord_media.exe, video-codec-probe.exe, discord_wer.dll
    krisp-*.kef                 Krisp voice-processing models (noise cancel / background voice cancel), shipped flat, not as a module
    modules/
      discord_clips-0/discord_clips/   one versioned module today: discord_clips.exe + its own onnxruntime.dll, msvcp140*.dll, vcruntime140*.dll
  modules/state.json            { "modules": {} } — empty; module state tracked here is separate from the on-disk modules/ folder above
  sunbird/
    debug-shell-token           32-byte token file (local dev/debug hook — name matches sunbird's own debug shell, confirming Phoenix is sunbird-derived)
    plugins/                    empty on this install
  app.lock, app.port, device_prefs.json(.lock), session.json, accounts.json, perf-run.json
  cache/, clips/, crashes/, logs/, store/, voice-data/, webview/
```

**Update scheme.** Unlike Electron Discord/Canary (`%LOCALAPPDATA%\Discord\app-<version>\`, updated by `Update.exe --processStart`, with the install registered to run `Update.exe`), Phoenix installs flat as `app\` with no version suffix in the path, and the registry uninstall entry (`HKCU\...\Uninstall\DiscordPhoenix`) points at `DiscordPhoenixSetup.exe --uninstall` directly rather than at a separate `Update.exe`. The per-module `modules/discord_clips-0/` naming (`<module>-<ver>`) suggests modules update independently of the main `app\Discord.exe`, tracked through `modules\state.json`; that file is empty on this install, so the mechanism that populates it during an actual update was not observed here. `DiscordSystemHelperPhoenix` is a separate installed Windows service (`C:\Program Files\Common Files\Discord\DiscordPhoenix\DiscordSystemHelper.exe service --channel phoenix`), parallel to the existing `DiscordSystemHelper` for Electron Discord — this is outside `%LOCALAPPDATA%` and will need its own entry in the installer's discovery/uninstall logic (2.2.6, 2.2.8).

**Answers to the task's "Done when" questions:**
- *Can `version.dll` sideload?* Not ruled out by anything in the PE header or manifest (`DependentLoadFlags = 0`, no `<dllDirectory>`). The binary does call `SetDefaultDllDirectories`, so an empirical sideload test (drop a stub `version.dll` next to `Discord.exe`, see if its `DllMain` runs) is still needed before 2.1.7 decides sideload vs. launcher — tracked as an open question below.
- *Which DLLs are imported?* 50, listed above; `version.dll`, `winhttp.dll`, `winmm.dll`, `dbghelp.dll`, `dwmapi.dll` present, `wininet.dll` absent, none delay-loaded.
- *What restrictions exist?* None recorded in the PE header (`DependentLoadFlags = 0`) or the manifest; whatever restriction exists (if any) is applied at run time via `SetDefaultDllDirectories`, not declared statically.

## Method and ground rules for §2.1.5–§2.1.7

These three sections close the binary-inspection parts of tasks 2.1.5 (allocator + HTTP transport) and 2.1.6 (gateway dispatcher ABI). They are derived from a local Ghidra dump of `Discord.exe` `0.1.0.2208` (`local_dumps/transport_dump.txt`), produced on the owner's machine and **gitignored**: it is a traffic/disassembly artefact that [`docs/AGENTS.md`](../AGENTS.md) ("Never commit … disassembly") and [`signatures/README.md`](../../signatures/README.md) forbid in-tree.

Consequences for what this note may contain, and does:

- **No raw bytes, no decompilation are reproduced here.** Every locator is given as a *masked* AOB signature — the allowed form, "a short byte mask plus a name" — derived from the dumped prologue by wildcarding each RIP-relative displacement, `call`/`jcc` rel32 and build-specific immediate. These are ready to become `signatures/*.toml` entries and to resolve through [`Found<T>`](../../crates/phoecord-loader/src/hooks/found.rs); `rip` is the byte offset of the operand to follow (signatures/README §Format).
- **Addresses** are VAs from the dump with the RVA alongside (image base `0x140000000`, so `RVA = VA − 0x140000000`). RVAs are what a `signatures` entry resolves to.
- **Absent ⇒ `unverified`.** This dump was sliced around the gateway event strings (`GUILD_CREATE`, `MESSAGE_CREATE`, `x-super-properties`): 24 functions, 39 string hits. It contains **no** `Transport`, `Request`, `Method`, `HeaderMap`, `reqwest`, `handle_alloc_error` or `__rust_dealloc` symbol. Anything the task asked for that is not physically in this dump is marked `unverified` and left for a wider dump, never guessed.

**Build fingerprint recovered from the dump** (symbolized data that survived stripping, useful as stable anchors for 2.2.1): a source-path prefix string `external\desktop\crates\ld_desk…` (`PTR_s_external__desktop_crates_ld_desk_*`, e.g. `…_156071c70`, `…_15634d900`), a rustc-commit marker `_rustc_73dc9167f1cd099e525c9ade2…` (`PTR_s__rustc_73dc9167f1cd099e525c9ade2_15601a178`), an `Arc counter overflow` panic string (`…_156019e30`), and symbolized `core::panicking::panic_fmt` / `core::option::expect_failed`. These confirm a stock Rust toolchain and give a per-build revision anchor (open question 8 in the plan, GPUI revision, is partly answerable this way: the `ld_desk` crate path and the rustc hash fingerprint the build even where no GPUI `Location` paths were captured in this slice).

## Allocator: `__rust_alloc` / `handle_alloc_error` (2.1.5)

The canonical Rust fallible-allocation idiom is present verbatim and twice over — `p = __rust_alloc(size, align); if p.is_null() { handle_alloc_error(layout) }` — at `0x149974960` (`FUN_149974960`) and `0x1469f4000` (`FUN_1469f4000`, the larger message handler). Both resolve the same two callees, which fixes the allocator shims:

| Shim | VA | RVA | Observed ABI (Win64) | Anchor status |
|:---|:---|:---|:---|:---|
| `__rust_alloc` | `0x14456d100` | `0x0456d100` | `rcx = size`, `rdx = align` → `rax = ptr` (null on failure). Call seen as `(0x20, 8)` = (size 32, align 8) | Address **verified** (two independent call sites); its own prologue is **not** in this dump, so no prologue AOB yet |
| `handle_alloc_error` | `0x1531145d0` | `0x131145d0` | noreturn; called `(8, 0x20)` = **(align, size)**, i.e. the reverse operand order of `__rust_alloc`. Reached only on the null branch, then `ud2` | Address **verified**; mapping of the two args to `Layout { size, align }` fields **inferred** from the sibling `__rust_alloc` call, not proven |
| `__rust_dealloc` | — | — | — | **unverified — not in this dump.** The `Box` free in `FUN_149974960` goes through drop glue `0x1483b4e20` (`FUN_1483b4e20`), not a bare `dealloc`; `Found<Allocator>.dealloc_rva` cannot be filled until a dump that reaches the `__rust_dealloc` shim is taken |

A neighbouring noreturn in the same `0x15311xxxx` page, `0x1531148e0` (`FUN_1531148e0`, RVA `0x131148e0`), is a *different* abort helper (it takes a `&str`-like `&UNK_…` argument, not a `Layout`); recorded so it is not mistaken for the alloc-error handler when scanning that page.

**`Found<Allocator>` anchor spec.** The allocator shims have no string xref and short, generic prologues (not dumped here), so the stable way in is the call-site idiom, not the shim entry. The owner's larger dump should cut the exact bytes; from the decompilation the call shape at `0x149974960` is:

```
; rcx=size, rdx=align per above
B9 20 00 00 00            mov  ecx, 0x20          ; size  (object-specific)
BA 08 00 00 00            mov  edx, 8             ; align (object-specific)
E8 ?? ?? ?? ??            call __rust_alloc        ; rip-rel → 0x0456d100
48 85 C0 / 48 83 F8 00    test rax,rax / cmp       ; null check
0F 84 / 75 ..             jz   fail
... fail:
B9 08 00 00 00            mov  ecx, 8             ; align
BA 20 00 00 00            mov  edx, 0x20          ; size
E8 ?? ?? ?? ??            call handle_alloc_error  ; rip-rel → 0x131145d0, noreturn
```

The two `size`/`align` immediates are per-allocation, so this exact window is **not** a reusable signature — mark these bytes `unverified` (reconstructed from the decompiler, not cut from the dump). The reusable locator for `signatures/*.toml` is: take the `E8 rel32` of either `call` and follow it to the shim entry (`rip`-style, but on a `call` rel32: `target = site + 5 + rel32`). The caller `FUN_149974960` *does* have a dumped, clean prologue to anchor the search on:

| Entry | AOB (masked, from the 32-byte prologue) | `rip` |
|:---|:---|:---|
| `alloc_callsite_host` (`FUN_149974960`, RVA `0x9974960`) | `55 56 57 48 83 EC 50 48 8D 6C 24 50 48 C7 45 F8 FE FF FF FF 4C 89 C7 48 89 CE 48 C7 45 D8 01 00` | — (then walk to the first `E8` whose target is `__rust_alloc`) |

**Answer to plan open-question 5 (allocator stability).** The `size`/`align` immediates and the exact null-check encoding are codegen-dependent and will move build to build; the two shim *addresses* are what matters, and the robust discovery is "scan for a `call rel32` landing on a function that is itself `call`ed from very many sites and never calls back" (the allocator's fan-in), or anchor on the `handle_alloc_error` fan-in, rather than any fixed byte window. This confirms the 2.2.2 fallback (local reply instead of body replacement) is the right default until the shims are pinned per build.

## HTTP transport: `dyn Transport` & `Request` layouts (2.1.5 / task's §2) — **largely unverified**

**This dump does not contain the HTTP transport.** Despite the file name, grepping the 24 dumped functions finds no `Transport`, `Request`, `Method`, `HeaderMap`, `reqwest`, URL or body symbol, and no vtable or send-future decompilation. The requested field offsets therefore **cannot be extracted from this dump and are all `unverified`** — they must not be guessed. What *is* present is two transport-adjacent anchors, recorded so the next (wider) dump can start from them:

| What | VA | RVA | Evidence | For |
|:---|:---|:---|:---|:---|
| `x-super-properties` header site | `0x14fc94406` | `0xfc94406` | Prologue only (no decompiled body). `lea r8,[rip+disp]` → the string, `mov r9d, 0x12` (length 18), `mov rcx,rdi; mov rdx,rsi; call …; test al,al; jnz` — the shape of a header-map insert/lookup keyed by `"x-super-properties"` | Finding the request **header map** build site in 2.2.1 |
| `create-invite` REST builder | `0x141a251e0` | `0x1a251e0` | `FUN_141a251e0` copies five scalar/pointer args into a stack record and calls `FUN_14199fb51(ctx, "create-invite"(len 0xd), &record, handle)` via `FUN_148511201(buf,"create-invite",0xd)` | A concrete REST route builder; a hook point candidate upstream of the transport |

**`Found<HttpSend>` / `Found<HttpPoll>` anchor spec — not derivable here.** `signatures` entries for `http_send` (entry of `ReqwestTransport::send`) and `http_poll` (the `poll` slot of the response-future vtable) need the transport code, which is absent. The one usable masked anchor from this slice is the header site, offered as a *waypoint*, not as either locator:

| Entry | AOB (masked, from the 32-byte prologue) | `rip` | Resolves to |
|:---|:---|:---|:---|
| `x_super_properties_site` (RVA `0xfc94406`) | `4C 8D 05 ?? ?? ?? ?? 41 B9 12 00 00 00 48 89 F9 48 89 F2 E8 ?? ?? ?? ?? 84 C0 0F 85 ?? ?? ?? ??` | `3` | the `"x-super-properties"` string (len `0x12`) |

To close 2.1.5's transport half, the owner needs a dump sliced on transport strings instead of gateway strings — candidate seeds: `reqwest`, `content-type`, `authorization`, a URL scheme literal, or the `ReqwestTransport`/`http::` type-name strings if they survive. Until then: `dyn Transport` vtable slot order, the `Request` field offsets (`Method`, URL, `HeaderMap`, body) and the send-future `poll` layout are **unverified**.

## Gateway dispatcher ABI (2.1.6)

The dump's core: a family of per-event dispatch functions that take a client-event name and route it. Their ABI is consistent across every dumped handler (`FUN_14baeadb0`, `FUN_1474f5590`, `FUN_147171230` @ `0x147171230`, `FUN_14723e460` @ `0x14723e460`, `FUN_147250360`), which pins the handler signature, the out-pointer, and the `Result`/poll niche.

**Handler signature (Win64).** The dispatch/handler entry is `fn(ctx, name_ptr, name_len, …)`:

| Reg | Arg | Meaning | Evidence |
|:---|:---|:---|:---|
| `rcx` | `param_1` | `&mut` context / store / sret out-pointer (see below) | written through, then passed on, in every handler |
| `rdx` | `param_2` | pointer to the event-name UTF-8 bytes | compared byte-wise and by hash |
| `r8` | `param_3` | event-name length | `== 0xe`, `== 0xc`, `== 8` length guards throughout `FUN_1474f5590`, `FUN_14baeadb0` |
| `r9`, stack | `param_4`, `param_5` | the event payload handle / cursor (opaque here) | forwarded unchanged into the matched sub-handler |

Name matching is done two ways, both present: (a) a **hashed set probe** `FUN_14af99f89(key, key_len, &hash, 1)` where the seed `hash = 0x2a` is spilled to the stack before each probe (`FUN_1474f5590`), and (b) **exact comparison** against entries of a single concatenated literal via `FUN_140043e35(name_ptr, name_len, "READYGUILD_CREATECHANNEL_CREATEMESSAGE_CREATE…", prefix_index)` plus inline 8-byte immediate compares — e.g. `*name == 0x5f4547415353454d` (`"MESSAGE_"`, little-endian) followed by `*(name+6) == 0x4554414552435f45` (`"E_CREATE"`). These inline event-name immediates are the **stable anchors** for a dispatcher signature (far more stable than the prologue), analogous to gpui.md §3's string-anchor method.

**Registry seed.** `FUN_147250360` (RVA `0x7250360`) builds/queries the registry: `FUN_1470fa2e0(ctx+0x18, "GUILD_CREATEGUILD_UPDATECHANNEL_CREATERELATIONSHIP_ADD…ENTITLEMENT_DELETE", 0xc, name, ctx, …)` — i.e. the registry lives at **`ctx + 0x18`** and is seeded from one concatenated event-name blob with per-entry lengths (`0xc` = len of `"GUILD_CREATE"`). The lock byte at `ctx + 0x10` (`LOCK … *(ctx+0x10) == 0` / set to `1`) is a `Once`/borrow flag guarding first-time registry init.

**Hidden out-pointer (sret) size.** Several handlers return their result by caller-allocated out-pointer in `rcx`, confirming an sret struct, and one pins the size: `FUN_141a566c0` copies **`0x88` = 136 bytes** from the source into `param_1` (`for (i=0x88; i; i--) *dst++ = *src++`) and returns `param_1`. `FUN_141a011b0` (GUILD_CREATE) returns `void`, writes through `param_1`, then drops `param_1 + 8`. So: **sret out-pointer = `rcx`, caller-allocated; one observed payload is 136 bytes**; other handlers return smaller records — the size is per-event, not a single constant (so a hook must treat `rcx` as an out-param of handler-specific size, not assume 136).

**`Result` / poll niche on Win64.** `FUN_147250360` returns a bare `ulonglong` whose low bits are a niche-packed discriminant, not an sret `Result`: the code branches on `uVar < 4`, `uVar & 2`, `uVar & 3`, `uVar & 0xfffffffffffffffc`, writing state tags `0/1/2/3` into the returned word and into a byte at `ctx+0x10`. This is the classic Rust **niche optimization packing a small enum (≈`Poll<…>` / a `Result` with a niche) into the low 2 bits of a pointer-sized word**: states `0,1,2,3` with the pointer payload masked by `& ~3`. The recurring `0xfffffffffffffffe` stack sentinel at every function entry is the **async/drop-state discriminant** (uninit marker `-2`), confirming these dispatchers are poll functions of a generated async state machine, not plain synchronous calls. Practical consequence for 2.2: a hook that reads a dispatcher's return value must mask `& 3` for the state and `& ~3` for the payload pointer; it must not expect a two-register `(ok, payload)` `Result`.

**`Found<GatewayRegistry>` anchor spec.** Anchor on the registry-build call in `FUN_147250360`, whose prologue is dumped, and verify by the `ctx+0x18` registry offset and the seed-blob reference:

| Entry | AOB (masked, from the 32-byte prologue) | `rip` | Note |
|:---|:---|:---|:---|
| `gateway_registry` (`FUN_147250360`, RVA `0x7250360`) | `55 56 57 48 83 EC 30 48 8D 6C 24 30 48 C7 45 F8 FE FF FF FF 48 89 D6 48 89 CF B1 01 31 C0 F0 0F` | — | prologue ends in `F0 0F…` = the `lock cmpxchg` on the `ctx+0x10` init flag; confirm by the `ctx+0x18` registry store and the `"GUILD_CREATE…ENTITLEMENT_DELETE"` seed-blob xref |

A more build-stable dispatcher anchor than any prologue is the inline event-name immediate pair `48 B8 4D 45 53 53 41 47 45 5F` (`mov rax, "MESSAGE_"`) near a `… "E_CREATE"` compare; recommend deriving the committed signature from that, per the string-anchor rule.

**Outgoing gateway send (plan open-question 3 / 2.2.4).** Not found in this dump — every dumped function is on the **inbound** (server→client dispatch) path. Whether an in-process client→server send function exists is **unverified**; needs a dump sliced on outbound opcodes or the gateway socket write. Likewise **READY handling** (open-question 4): `READY` appears only as the first entry of the inbound seed blob (`"READYGUILD_CREATE…"`), so on this evidence READY is dispatched through the same registry as other events, but whether an *additional* out-of-band READY path exists is **unverified**.

## Open questions (carried to 2.1.5–2.1.7)

- Does the observed `SetDefaultDllDirectories` call actually exclude the application directory before `version.dll` would be probed? Needs an empirical sideload test or a debugger breakpoint; this read-only PE pass cannot answer it.
- Is Control Flow Guard (`GUARD_CF`, confirmed above) enforced strictly enough to break an inline detour that doesn't target a CFG-valid entry? Needs a prototype hook test (2.1.5/2.1.6).
- How does `modules/state.json` get populated during a real update, and does the main `app\Discord.exe` itself update in place or move to a new path? Not observed on this fresh install; revisit after an actual Phoenix auto-update.
- `DiscordSystemHelperPhoenix` runs from `C:\Program Files\Common Files\Discord\DiscordPhoenix\`, outside `%LOCALAPPDATA%` — the installer's Phoenix discovery (2.1.1/2.2.6) should account for this second location, not just the per-user app folder.
- **HTTP transport not in the current dump.** `dyn Transport` / `Request` field offsets (`Method`, URL, `HeaderMap`, body), the vtable slot order and the send-future `poll` layout are all `unverified`; a transport-sliced dump (seeds: `reqwest`, `content-type`, `authorization`, a URL scheme, `http::`) is needed to close the transport half of 2.1.5.
- **`__rust_dealloc` not in the current dump.** Its RVA (for `Found<Allocator>.dealloc_rva`) is `unverified`; a dump reaching the dealloc shim is needed before body replacement (2.2.2) can be enabled rather than falling back to local reply.
- **Outgoing gateway send and an out-of-band READY path** were not observed (every dumped function is inbound dispatch); both `unverified`, feeding plan open-questions 3 and 4 and task 2.2.4.
- **2.1.7 is not closed by this note.** The loading-path decision (sideload vs launcher) and the hook-timing prototype are a build-and-test task, not documentation; the CFG-enforcement and `SetDefaultDllDirectories` questions above still gate it.
