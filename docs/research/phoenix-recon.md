# Desktop Phoenix reconnaissance (stage 2.1)

Findings about an official Phoenix build, and the steps that produce them. This file holds the sections of tasks **2.1.3** (proxy behavior), **2.1.4** (gateway wire format) and **2.3.7** (degradation). Task 2.1.9 assembles the full report around them: binary layout, loading path, HTTP transport, gateway registry and the decisions for 2.2–2.4.

**Status: method only. Nothing here has been run against Phoenix.** Every result cell says "not run" until someone with access fills it in. What these steps expect is a hypothesis, not a finding. What the study of another loader says about Phoenix is in [`phoenix-desktop.md`](phoenix-desktop.md).

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
| Date, OS | not run |
| Phoenix version | not run |
| Executable SHA-256 | not run |

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

### Step 1. `HTTPS_PROXY`

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

### Step 2. System proxy

Close the terminal that set `HTTPS_PROXY`, or run `Remove-Item Env:HTTPS_PROXY`.

```powershell
$k = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
Get-ItemProperty $k | Select-Object ProxyEnable, ProxyServer, AutoConfigURL   # write these down
Set-ItemProperty $k ProxyServer '127.0.0.1:8080'
Set-ItemProperty $k ProxyEnable 1
```

Start Phoenix from the Start menu and watch mitmproxy. Then restore the values you wrote down (`Set-ItemProperty $k ProxyEnable 0` if the proxy was off).

If this fails, the client may read the WinHTTP setting instead (elevated shell): `netsh winhttp show proxy`, `netsh winhttp set proxy 127.0.0.1:8080`, and afterwards `netsh winhttp reset proxy`.

Linux: `http_proxy` and `https_proxy` in the environment; on GNOME also the system proxy setting.

### Step 3. System certificate store

Use whichever method of step 1 or 2 delivered traffic. First without any added CA: every intercepted handshake must fail. If requests succeed, the client does not verify certificates; record that and tell the owner.

Then PhoeCord's own CA, which is what the fallback uses:

```powershell
phoecord trust                       # CurrentUser\Root
$env:PHOECORD_LOG = 'phoecord=debug'
phoecord run                         # 127.0.0.1:18463
```

```powershell
$env:HTTPS_PROXY = 'http://127.0.0.1:18463'
Start-Process 'C:\path\to\Phoenix\Discord.exe'
```

| `phoecord run` shows | Meaning |
|:---|:---|
| `gateway connected (…)` | The proxy is used and the CA is trusted |
| `gateway.discord.gg:443: …` with a TLS alert such as `UnknownCA` | The proxy is used, the certificate is refused |
| Nothing | No intercepted host came here: compare the hosts in step 1 with the `intercept` list in `config.toml` |

The proxy speaks HTTP/1.1 only. If the client requires HTTP/2, record the error.

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
| `HTTPS_PROXY` honoured | not run | not run |
| `--proxy-server` honoured | not run | not run |
| System proxy honoured | not run | not run |
| Verifies certificates | not run | not run |
| PhoeCord CA in the system store accepted | not run | not run |
| mitmproxy CA in the system store accepted | not run | not run |
| TLS crates seen | not run | not run |
| HTTP/2 or QUIC required | not run | not run |
| Hosts through the proxy | not run | not run |
| Hosts that bypass it | not run | not run |

### What the answers mean for 2.1.8

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
gateway connected (Etf, Zstd)
```

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

Run the same capture against today's Electron client (`Discord.exe --proxy-server=http://127.0.0.1:8080`) with the same account and actions, for the comparison.

### Fixtures

`tests/fixtures/phoenix/` gets at least `HELLO`, `READY` and `MESSAGE_CREATE`, decoded. They are **rewritten, not redacted**: keep keys, nesting and value types; replace every string, every snowflake (including snowflakes used as map keys) and every number that identifies something with invented values. Delete nothing from the shape. `READY` carries the email, phone, session ids, resume URL, analytics token, settings, relationships and server names; none of it may reach the repository.

Decoding the capture and rewriting it is the second half of 2.1.4. It is not written yet, because it depends on the encoding found above.

### Event shapes

For each event name, compare the set of key paths and value types between the two captures. Key paths contain no personal data once snowflake keys are replaced.

### Results

| Question | Phoenix | Electron today |
|:---|:---|:---|
| Encoding | not run | not run |
| Compression | not run | not run |
| Gateway version (`v=`) | not run | not run |
| Other URL parameters | not run | not run |
| Decodes with the current codec | not run | not run |
| Events only on this client | not run | not run |
| Fields added or removed | not run | not run |

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
