Checked against code: not applicable (no PhoeCord code touches Phoenix yet); against a local install of Discord Phoenix, file version `0.1.0.2208`, read 2026-11-25/26 (part 1 includes a static pass of the installed binary on 2026-11-26). Part 1's exact addresses, byte signatures and structure offsets are kept in the team's private recon notes, not in this file (ask Nanda for access); this file keeps everything that is useful without them, plus all of part 2.

# Desktop Phoenix reconnaissance (stage 2.1)

Findings about an official Phoenix build, and the steps that produce them. Part 1 holds tasks **2.1.2** (binary inspection), **2.1.5** and **2.1.6** (allocator, HTTP transport, gateway dispatcher); part 2 holds **2.1.3** (proxy behavior), **2.1.4** (gateway wire format) and **2.3.7** (degradation). Task 2.1.9 completes the report: loading path and the decisions for 2.2–2.4.

**Status of part 2: partly run (2026-11-25, Windows).** Proxy behavior and the gateway encoding and compression have Windows results. The capture, fixtures, event comparison, Linux and degradation are "not run". What an unrun step expects is a hypothesis, not a finding.

**The account used for the signed-in runs is not accepted by this client.** It reaches `READY` and then shows "This app isn't available for your account". Signed-in results cover only what the client sends and receives up to and on that screen. Nothing in the traffic was changed and no way past the screen was tried. What the study of another loader says about Phoenix is in [`phoenix-desktop.md`](phoenix-desktop.md).

# Part 1. The binary (2.1.2, 2.1.5, 2.1.6)

Facts in part 1 come from a local install, read with read-only tools (`phoecord dev inspect-pe`, PowerShell, `Get-AuthenticodeSignature`, resource APIs used only to read the manifest) and a static pass over the installed binary (headless Ghidra on a private project, never run against Phoenix). No sunbird code or binaries are reproduced (see [`phoenix-desktop.md`](phoenix-desktop.md) §0). **Exact addresses, byte signatures and structure offsets from this static pass are not published here**: they are reverse-engineering detail about a specific shipped build and live in the team's private recon notes instead (consistent with [`docs/AGENTS.md`](../AGENTS.md), "Never commit … disassembly" — the same rule that keeps `local_dumps/transport_dump.txt` gitignored). What follows is everything useful without them.

## Binary inspection (2.1.2)

**Target:** `C:\Users\<user>\AppData\Local\DiscordPhoenix\app\Discord.exe`, 518,388,664 bytes, `FileVersion`/`ProductVersion` `0.1.0.2208`, `ProductName` "Discord Phoenix", signed by Discord Inc. (valid Authenticode signature; so are every other `.exe`/`.dll` directly under `app\`).

**PE headers.** PE32+ (x86-64), Windows GUI subsystem. No .NET (CLR) metadata — confirms a native (Rust) client, not Electron/.NET. `DllCharacteristics = 0xc160`: `HIGH_ENTROPY_VA | DYNAMIC_BASE | NX_COMPAT | GUARD_CF | TERMINAL_SERVER_AWARE`. `GUARD_CF` means Control Flow Guard is on — an inline detour must still produce valid CFG-reachable targets or the hooked thread's indirect calls through hooked pointers can crash under CFG bitmap enforcement; this needs to be verified in 2.1.5/2.1.6 when the actual detour is written.

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

**TLS and sections.** One TLS directory with 4 callbacks (typical of Rust's TLS-based thread-local destructors/initializers, not evidence of anti-tamper). 15 sections; besides the usual `.text`/`.rdata`/`.data`/`.pdata`/`.reloc`, there are Rust/LLVM-specific sections `.eh_fram`(e), `.fptable`, `.itt_not`(ify, Intel VTune instrumentation — a debug/profiling leftover, not stripped from this build), `.xdata`, and two Intel-IPP-library sections `IPPCODE`/`IPPDATA` (`r-x`/`rw-` — a statically linked codec or crypto library using Intel's Integrated Performance Primitives). The full section table (virtual addresses, raw offsets and sizes), which is the base for AOB scans in 2.1.5/2.1.6/2.1.7, is in the private recon notes.

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

## Allocator, HTTP transport and Gateway dispatcher — findings, no addresses (2.1.5, 2.1.6)

Two static passes were made over the installed binary (2026-11-25 sliced on gateway strings; 2026-11-26 wider, with a working cross-reference index). Nothing was run inside Phoenix, so none of this is confirmed at run time. The addresses, byte signatures (for `signatures/*.toml`) and exact structure offsets behind every bullet below are in the team's private recon notes, kept there because they are reverse-engineering detail about one specific shipped build (not source code, not something that should sit in a public history of a project that also tells contributors never to commit disassembly).

- **Allocator (2.1.5).** The host's `__rust_alloc` and `handle_alloc_error` shims were located from the fallible-allocation call idiom, confirmed at two independent call sites. `__rust_dealloc` was also located on the second pass (its call-site fan-in is far larger than the allocator's own, as expected). A masked, build-specific call-site signature exists for the allocator entry; the shim entries themselves have no string cross-reference, so the robust discovery is "the function most other functions call but which itself never calls back", not a fixed byte window. This confirms the 2.2.2 fallback (local reply instead of body replacement) is the right default until the shims are pinned per build.
- **HTTP transport (2.1.5).** The first pass's dump did not contain the transport at all (it was sliced on gateway strings). The second pass located the `send` entry of the `reqwest`-based transport, its vtable, the response future's `poll` slot, and the in-memory layout of the request it builds (headers, body, URL, timeout, method field inferred). Each has a masked signature that matches uniquely in this build's `.text`, with enough leading whole instructions for an inline detour; none is checked on a second build yet, so none is in `signatures/` (2.2.9 blocks that). The response layout (status, headers, body) that `on_response` needs is still open, and so is everything at run time — nothing here has been exercised inside the client.
- **Gateway dispatcher (2.1.6).** The inbound per-event dispatch functions share one ABI: a context/out-pointer, the event name as a pointer+length, and a payload handle, with the result packed as a niche-optimized small enum rather than a two-register `Result`. A registry build/query call was found and anchored, though the task's "done when" (the registry table's exact address and entry layout) is not fully met yet. The **second pass additionally found an outgoing (client→server) send path** — this answers plan open question 3 and gives 2.2.4 something concrete to hook — and opcodes consistent with known Gateway operations (presence, voice state, guild member requests, and others); heartbeat, identify and resume are not produced by it, so where those are sent is still open. The client's connection loop treats the `READY` event name specially in four places; whether its payload also still goes through the ordinary event registry, or only this special path, is unverified.

**Open for 2.1.6, carried to the next pass:** the registry table's address and entry layout.
**Open for 2.1.5, carried to 2.2.2:** the response layout, and everything at run time (Control Flow Guard enforcement on a real detour has not been tested).

# Part 2. Proxy, gateway, degradation (2.1.3, 2.1.4, 2.3.7)

## Before you start

- **Risk.** These steps run a modified network path against a live account. Client modification and traffic interception break Discord's Terms ([`TERMS.md`](../../TERMS.md)). Use an account you can afford to lose.
- **Secrets.** Captured traffic contains the account token and personal data. Keep captures in a folder outside the repository and delete them when the findings are written down.
- **Trusted certificates.** The mitmproxy CA, unlike PhoeCord's, has no name constraint: while it is trusted, whoever holds its key can impersonate any site to you. Remove it as soon as the CA steps are done.
- **One client.** Quit Discord completely (tray icon too) before every launch. A second start hands over to the running process, which keeps its old environment.

Tools: a built `phoecord`, [mitmproxy](https://mitmproxy.org) (`winget install mitmproxy.mitmproxy`), and for the Linux steps `ss`.

Record the build first. With the loader installed, `%LOCALAPPDATA%\PhoeCord\loader.log` has the line `client build: version … .text sha256 …`. Without it:

```powershell
$exe = (Get-Process Discord* | Select-Object -First 1).Path   # while the client runs
(Get-Item $exe).VersionInfo.FileVersion
Get-FileHash $exe -Algorithm SHA256
```

| Fact | Value |
|:---|:---|
| Date, OS | 2026-11-25, Windows 11 25H2 (26200) |
| Phoenix version | file 0.1.0.2208; release `0.1.0-dev.20261123093910.gb71ec80`, channel `release/phoenix_desktop`, client build 678271 |
| Executable SHA-256 | `f4186cf28cb5289c0e6764de11eef8f91cb205b72284050582aa29b3c0e5ca33` |
| Signature | valid, Discord Inc. |
| Executable | `%LOCALAPPDATA%\DiscordPhoenix\app\Discord.exe` (the folder has no version in its name) |

The client logs to `%LOCALAPPDATA%\DiscordPhoenix\logs\libdiscord.log`, one JSON object per line; TLS failures show up there. Redact before quoting it.

## Proxy behavior

The questions: does Phoenix honour `HTTPS_PROXY`, does it honour the system proxy, does it trust the system certificate store, and does it pin. Task 2.1.8 picks the proxy reach option from the answers.

### How to tell where a connection went

Run this while the client is connected. A remote address of `127.0.0.1` on the proxy's port means that connection used the proxy; a public address on port 443 means it went around.

```powershell
$ids = (Get-Process Discord*).Id
Get-NetTCPConnection -State Established -OwningProcess $ids |
  Where-Object RemotePort -in 443, 8080, 18463 |
  Select-Object OwningProcess, RemoteAddress, RemotePort
```

Linux: `ss -tnp | grep -i discord`.

QUIC (UDP) would not appear here and would not pass an HTTP proxy. Check `Get-NetUDPEndpoint -OwningProcess $ids` as well; voice uses UDP too, so look before joining a call.

### Step 1. `HTTPS_PROXY` and `--proxy-server`

Start with the environment variable alone. `phoecord launch` also passes `--proxy-server`, which would hide which of the two worked.

```powershell
mitmweb --listen-port 8080          # terminal 1
```

```powershell
$env:HTTPS_PROXY = 'http://127.0.0.1:8080'   # terminal 2
Start-Process 'C:\path\to\Phoenix\Discord.exe'
```

Linux: `HTTPS_PROXY=http://127.0.0.1:8080 ./Discord`.

mitmproxy's list shows each host the client asks for, even when the TLS handshake then fails. A request arriving here answers the question; the certificate is step 3.

If nothing arrives, repeat with `https_proxy`, then `ALL_PROXY`, then the command-line flag `--proxy-server=http://127.0.0.1:8080`.

**Result.** With `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` all set (checked to reach a child process started the same way), nothing arrived and the client connected directly. With `--proxy-server` every TCP connection went to the proxy: `discord.com` and `status.discord.com` (ALPN `h2`, `http/1.1`) and `remote-auth-gateway.discord.gg` (no ALPN). A tunnel that decrypts nothing was enough for this step.

### Step 2. System proxy

Close the terminal that set `HTTPS_PROXY`, or run `Remove-Item Env:HTTPS_PROXY`.

```powershell
$k = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
Get-ItemProperty $k | Select-Object ProxyEnable, ProxyServer, AutoConfigURL   # write these down
Set-ItemProperty $k ProxyServer '127.0.0.1:8080'
Set-ItemProperty $k ProxyEnable 1
```

**Result.** Honoured: with only the registry values set (followed by `InternetSetOption` 39 and 37, and the effective value checked with `WinHttpGetIEProxyConfigForCurrentUser`), the client's connections, gateway included, went to the proxy.

Start Phoenix from the Start menu and watch mitmproxy. Then restore the values you wrote down (`Set-ItemProperty $k ProxyEnable 0` if the proxy was off).

If this fails, the client may read the WinHTTP setting instead (elevated shell): `netsh winhttp show proxy`, `netsh winhttp set proxy 127.0.0.1:8080`, and afterwards `netsh winhttp reset proxy`.

Linux: `http_proxy` and `https_proxy` in the environment; on GNOME also the system proxy setting.

### Step 3. System certificate store

Use whichever method of step 1 or 2 delivered traffic (on this build: `--proxy-server`). First without any added CA: every intercepted handshake must fail. If requests succeed, the client does not verify certificates; record that and tell the owner.

Then PhoeCord's own CA, which is what the fallback uses:

```powershell
phoecord trust                       # CurrentUser\Root
$env:PHOECORD_LOG = 'phoecord=debug'
phoecord run                         # 127.0.0.1:18463
```

```powershell
Start-Process 'C:\path\to\Phoenix\Discord.exe' -ArgumentList '--proxy-server=http://127.0.0.1:18463'
```

| `phoecord run` shows | Meaning |
|:---|:---|
| `gateway connected (…)` | The proxy is used and the CA is trusted |
| `discord.com:443: …` with a TLS alert such as `UnknownCA` | The proxy is used, the certificate is refused |
| Nothing | No intercepted host came here: compare the hosts in step 1 with the `intercept` list in `config.toml` |

The proxy speaks HTTP/1.1 only. If the client requires HTTP/2, record the error.

**Result without the CA trusted.** `phoecord run` showed `discord.com:443: received fatal alert: UnknownCA`; the client logged `rustls_platform_verifier::verification::windows` / `invalid peer certificate: UnknownIssuer`. So the client verifies certificates, through the Windows verifier.

**Result with the CA trusted** (`phoecord trust`, removed afterwards): the connections to `discord.com` and `gateway.discord.gg` were intercepted and worked over HTTP/1.1, the proxy logged `gateway connected (Json, Zstd)`, and the client logged `signed in: READY`. No pin showed on those two hosts, so step 4 was not needed.

### Step 4. Bundled roots or pinning

Do this only if step 3 refused PhoeCord's CA. Repeat step 3 with mitmproxy's CA, to rule out the name constraint:

```powershell
certutil -user -addstore Root "$env:USERPROFILE\.mitmproxy\mitmproxy-ca-cert.cer"
# ... launch through port 8080, look, then remove it:
certutil -user -delstore Root mitmproxy
```

If mitmproxy's CA works and PhoeCord's does not, the client's verifier rejects something about our certificate (name constraints are the first suspect).

If both are refused, **that alone does not prove pinning.** A client with its own root set (for example the `webpki-roots` crate) refuses a system-trusted CA too. The two differ in what 2.1.8 can do: a root set can be extended in-process (option 3), a pin cannot be satisfied by any CA of ours. Look for the TLS stack's crate paths, which Rust binaries often keep in panic messages:

```bash
grep -a -o -E '(rustls|rustls-platform-verifier|rustls-native-certs|webpki-roots|native-tls|schannel|reqwest|hyper-util)-[0-9]+\.[0-9]+\.[0-9]+' Discord.exe | sort -u
```

A name found is evidence that the crate is linked; a name not found proves nothing. Record the output (crate names and versions only, no other bytes of the binary). Telling a pin from a root set for certain needs the verifier located in the binary (with 2.1.5).

Afterwards: `phoecord untrust` if this machine does not otherwise run PhoeCord.

### Step 5. Endpoints

With a working setup, use the client for a few minutes (open channels, send a message, upload a file, open settings) and list which hosts went through the proxy and which connected directly.

### Results

| Question | Windows | Linux |
|:---|:---|:---|
| `HTTPS_PROXY` honoured | **no** (nor `HTTP_PROXY`, `ALL_PROXY`) | not run |
| `--proxy-server` honoured | **yes** | not run |
| System proxy honoured | **yes** | not run |
| Verifies certificates | **yes**, `rustls-platform-verifier` (Windows) | not run |
| PhoeCord CA in the system store accepted | **yes** (`CurrentUser\Root`), on `discord.com` and `gateway.discord.gg` | not run |
| mitmproxy CA in the system store accepted | not needed | not run |
| TLS crates seen | `rustls` 0.23.46, `rustls-platform-verifier` 0.7.1, `rustls-native-certs` 0.8.5, `aws-lc-rs` 1.18.2; no `webpki-roots` path | not run |
| HTTP/2 or QUIC required | HTTP/2 not required; no UDP seen outside voice (voice not tested); `h2` 0.4.20 and `quinn` 0.11.9 are linked | not run |
| Hosts through the proxy | intercepted: `discord.com`, `gateway.discord.gg`; tunnelled: `status.discord.com`, `cdn.discordapp.com`, `latency.discord.media`, `remote-auth-gateway.discord.gg` (signed out), connected-account services | not run |
| Hosts that bypass it | `o64374.ingest.sentry.io` (error reporting), with the flag and with the system proxy | not run |

### What the answers mean for 2.1.8

On Windows the first row applies, through `--proxy-server` or the system setting rather than `HTTPS_PROXY`: no hook is needed. `phoecord launch` already passes the flag; a client started any other way needs the flag in its shortcut or the system setting. The ruling is the owner's and is not recorded yet.

| Proxy setting | System CA | Option (`ARCHITECTURE.md`, "Phoenix and the proxy") |
|:---|:---|:---|
| Honoured | Accepted | 1: nothing to build |
| Ignored | Accepted | 2: the loader sets the proxy |
| Honoured | Refused, own root set | 3: the loader adds our CA to the process roots |
| Ignored | Refused, own root set | 2 **and** 3: the options are not exclusive |
| Any | Refused because of a pin | None of the three: the proxy cannot be the fallback, which needs an owner ruling |

Options 2 and 3 are hooks, so they need signatures for each build, the same thing whose absence makes the proxy the fallback. A fallback that depends on them does not cover an unknown build; the ruling for 2.1.8 has to say so.

## Gateway wire format

This section matters for the proxy fallback only: in-process, the registry hands out decoded JSON ([`phoenix-desktop.md`](phoenix-desktop.md) §3.2).

### Encoding and compression

With `phoecord run` working (step 3), the answer is the line it prints for each connection:

```
gateway connected (Json, Zstd)
```

The line appears only while an enabled plugin reads the gateway; otherwise the proxy passes the connection through without decoding and says nothing. For an observation run, enable `message_logger`, which changes nothing at start-up, and disable `no_track`, so that no request is answered locally.

`phoecord-proxy/src/codec.rs` reads `encoding` (`json`, `etf`) and `compress` (`zlib-stream`, `zstd-stream`, or none) from the gateway URL. Anything else prints `unknown gateway …; passing the gateway through untouched`, and that line is the finding: it names what the codec lacks. Also note a connection that is logged as connected but then fails to decode: the parameters may be familiar while the framing is not.

### Capture

With a CA the client accepts, save the server's frames with this addon. It skips client frames on purpose: `IDENTIFY` and `RESUME` carry the token. Save it outside the repository as `gateway_capture.py`:

```python
"""mitmproxy addon: raw server -> client gateway frames, one JSON line each."""
import base64, json, os, pathlib

OUT = pathlib.Path(os.environ["PHOENIX_CAPTURE"])


class Capture:
    def websocket_message(self, flow):
        host = flow.request.pretty_host
        if not (host == "gateway.discord.gg" or host.startswith("gateway-")):
            return
        msg = flow.websocket.messages[-1]
        if msg.from_client:
            return
        line = {"path": flow.request.path, "text": msg.is_text,
                "data": base64.b64encode(msg.content).decode()}
        with (OUT / f"{flow.id}.jsonl").open("a", encoding="utf-8") as f:
            f.write(json.dumps(line) + "\n")


addons = [Capture()]
```

```powershell
$env:PHOENIX_CAPTURE = "$env:USERPROFILE\phoenix-capture"
New-Item -ItemType Directory -Force $env:PHOENIX_CAPTURE
mitmdump --listen-port 8080 -s gateway_capture.py
```

Start the client after mitmdump: stream compression keeps one context per connection, so a capture that misses the first frame cannot be decompressed. In the client, wait for it to load and send one message, then quit.

Start Phoenix with `--proxy-server=http://127.0.0.1:8080`.

Run the same capture against today's Electron client (`Discord.exe --proxy-server=http://127.0.0.1:8080`) with the same account and actions, for the comparison.

### Fixtures

`tests/fixtures/phoenix/` gets at least `HELLO`, `READY` and `MESSAGE_CREATE`, decoded. They are **rewritten, not redacted**: keep keys, nesting and value types; replace every string, every snowflake (including snowflakes used as map keys) and every number that identifies something with invented values. Delete nothing from the shape. `READY` carries the email, phone, session ids, resume URL, analytics token, settings, relationships and server names; none of it may reach the repository.

Decoding the capture and rewriting it is the second half of 2.1.4. It is not written yet, because it depends on the encoding found above.

### Event shapes

For each event name, compare the set of key paths and value types between the two captures. Key paths contain no personal data once snowflake keys are replaced.

### Results

| Question | Phoenix | Electron today |
|:---|:---|:---|
| Encoding | **JSON** | not run |
| Compression | **zstd-stream** | not run |
| Gateway version (`v=`) | not run | not run |
| Other URL parameters | not run | not run |
| Decodes with the current codec | **yes** | not run |
| Events only on this client | not run | not run |
| Fields added or removed | not run | not run |

No `undecodable gateway payload` line appeared at debug level during about 45 seconds of one connection, so no change to `codec.rs` is needed for what was seen. The URL parameters are not logged, and the client's full event traffic was not seen (see the status of part 2).

## Degradation

Task 2.3.7. Scenarios 1 and 2 need the locators (2.2.5) and the in-process transport (2.2.7); until then only what the loader does today can be observed. The decision behind them (which entry points and features stay on) is already tested with fake locator results in `phoecord_loader::plan`. Install it with `phoecord setup --loader` and read `%LOCALAPPDATA%\PhoeCord\loader.log`.

| # | Scenario | How to produce it | Pass when | Result |
|:---|:---|:---|:---|:---|
| 1 | Unknown build | No table for this build in `%LOCALAPPDATA%\PhoeCord\signatures\` (an empty folder, or only other builds) | Log: `unknown client build: UI layer stays off, network plugins unaffected`; the client runs; network plugins work through the proxy | not run |
| 2 | One signature missing | A table for this build with one pattern that matches nothing | Log: `signature <name>: no match; its feature stays off`; only that feature is off | not run |
| 3 | Electron client | The loader beside today's `Discord.exe` | No hooks; the client behaves as without the loader; no crash over a normal session | not run |

For scenario 2, `signatures\broken.toml` with the version from the `client build:` log line:

```toml
[build."0.0.0"]   # replace with the logged version
missing_on_purpose = { aob = "DE AD BE EF DE AD BE EF DE AD BE EF" }
```

In scenario 3 the loader today does not tell Electron from Phoenix: it finds no signatures and leaves everything off. Whether that is enough, or the loader must recognise Phoenix first, is decided once 2.1.2 shows what identifies it.

## Open questions (carried to 2.1.5–2.1.7)

- Does the observed `SetDefaultDllDirectories` call actually exclude the application directory before `version.dll` would be probed? Needs an empirical sideload test or a debugger breakpoint; this read-only PE pass cannot answer it.
- Is Control Flow Guard (`GUARD_CF`, confirmed above) enforced strictly enough to break an inline detour that doesn't target a CFG-valid entry? Needs a prototype hook test (2.1.5/2.1.6).
- How does `modules/state.json` get populated during a real update, and does the main `app\Discord.exe` itself update in place or move to a new path? Not observed on this fresh install; revisit after an actual Phoenix auto-update.
- `DiscordSystemHelperPhoenix` runs from `C:\Program Files\Common Files\Discord\DiscordPhoenix\`, outside `%LOCALAPPDATA%` — the installer's Phoenix discovery (2.1.1/2.2.6) should account for this second location, not just the per-user app folder.
- **HTTP transport response layout.** `send`, its vtable and the request layout were located on the second pass (see the summary above); the response layout (status, headers, body) that `on_response` needs is still open, and none of it is confirmed at run time.
- **Gateway registry table.** Its address and entry layout (what 2.1.6's "done when" asks for) are still open after both passes.
- **READY and the event registry.** The connection loop handles `READY` specially; whether its payload also still goes through the ordinary event registry is unverified.
- **2.1.7 is not closed by this note.** The loading-path decision (sideload vs launcher) and the hook-timing prototype are a build-and-test task, not documentation; the CFG-enforcement and `SetDefaultDllDirectories` questions above still gate it.
