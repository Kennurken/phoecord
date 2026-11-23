# Аудит Ashcord

Разбор [Tjabooo/ashcord](https://github.com/Tjabooo/ashcord) на коммите `5ade8b9` (0.1.0, 10.11.2026), MIT: что берём в фундамент PhoeCord, что переписываем и куда переезжает каждый модуль.

---

## Что это за код

Один бинарный крейт, ~4200 строк Rust, edition 2024. tokio + hyper 1 + rustls 0.23 (ring) + tokio-tungstenite. Работает как локальный HTTPS-прокси (`127.0.0.1:18463`): на `CONNECT` к хостам Discord завершает TLS сертификатом локального CA, остальное туннелирует байт в байт. Плагины зашиты в бинарник, управление — через CLI и `config.toml` с live-reload.

## Берём как фундамент

| Модуль | Строк | Почему берём | Куда |
|:---|---:|:---|:---|
| `val.rs` | 209 | Один тип для JSON и ETF, сохраняет атомы, big-int, бинарники — payload из ETF кодируется обратно без потерь | `phoecord-core` ✅ |
| `etf.rs` | 358 | Erlang External Term Format, на нём работает десктопный клиент | `phoecord-core` ✅ |
| `codec.rs` | 246 | zlib-stream и zstd-stream с общим контекстом на соединение, перекодирование своим контекстом | `phoecord-proxy` ✅ |
| `ca.rs` | 226 | CA на каждую установку, X.509 name constraint на домены Discord, кэш leaf-сертификатов по SNI, тесты на constraint | `phoecord-proxy` ✅ |
| `trust.rs` | 110 | Установка/удаление CA: Windows (`certutil -user`), macOS (keychain), Linux (NSS, включая Flatpak и Snap) | `phoecord-proxy` ✅ |
| `proxy.rs` | 115 | `CONNECT`, только loopback, MITM только для списка `intercept` | `phoecord-proxy` 🔄 |
| `rest.rs` | 206 | keep-alive к upstream, hop-by-hop заголовки, JSON-тело только когда плагину нужно | `phoecord-proxy` 🔄 |
| `gateway.rs` | 250 | Мост websocket клиент ↔ Discord, декодирование только при активных gateway-плагинах | `phoecord-proxy` 🔄 |
| `state.rs` | 244 | Знания об аккаунте из READY: user id, гильдии, эмодзи, имена | `phoecord-proxy` (за `Host`) 🔄 |
| `selftest.rs` | 160 | Проверка против настоящего Discord без логина | `phoecord-cli` ⏳ |

## Переписываем

| Что | Проблема | Решение в PhoeCord |
|:---|:---|:---|
| `plugins/mod.rs` — trait `Plugin` | 8 методов-хуков, `Cx` с `&mut Account` и `&Config` наружу: не переносится через границу WASM и привязывает плагин к внутренностям | Plugin API v1: `Manifest` + `wants(EventKind)` + `on_event(Event) -> Verdict`, всё внешнее — через `Host` (`phoecord-core::plugin`) |
| `plugins::all()` | Список плагинов зашит в бинарник, новый плагин = пересборка | `phoecord-plugin-runtime::Chain` + загрузка WASM-пакетов из папки плагинов |
| `config.rs` | Типизированная структура с полем на каждый плагин: новый плагин требует правки ядра | Секция `[plugins.<id>]` как свободная таблица, схема настроек — в манифесте плагина |
| `app.rs` | Глобальный `Mutex<Account>` держится на время всей цепочки | `Host` с короткими блокировками, состояние плагина — в его KV |
| `main.rs` — CLI | Вся логика в одном файле, setup/trust/launch смешаны | `phoecord-cli` (тонкий) + `phoecord-installer` (всё, что меняет систему) |
| `launch.rs` | Только `--proxy-server` и `HTTPS_PROXY` | Остаётся для Electron; для Phoenix — загрузчик или системный прокси, по результатам исследования (ROADMAP 1.6) |
| `vencord.rs` | Маппинг Vencord → Ashcord захардкожен | Поле `vencord` в манифесте плагина |
| `notify.rs` | `notify-rust` вызывается из плагинов напрямую | `Host::notify` с rate limit |

## Плагины

| Ashcord | Строк | Слой | Статус переноса |
|:---|---:|:---|:---|
| `no_track` | 41 | REST | ✅ на Plugin API v1 |
| `silent_typing` | 42 | REST | ⏳ 1.2 |
| `unindent` | 98 | REST | ⏳ 1.2 |
| `game_activity_toggle` | 61 | Gateway send | ⏳ 1.2 |
| `relationship_notifier` | 131 | Gateway dispatch | ⏳ 1.2 |
| `fake_nitro` | 157 | REST + Gateway | ⏳ 1.2 |
| `message_logger` | 358 | Gateway dispatch | ⏳ 1.2 — отображение удалённых пока через подмену события, в Phase 2 — через UI-хук |

## Найдено при переносе

- `trust.rs` импортировал `PathBuf` безусловно, а используется он только на Linux: на Windows и macOS сборка с `-D warnings` падала. CI Ashcord гоняет только `ubuntu-latest`, поэтому это не ловилось. Исправлено; CI PhoeCord запускается на трёх ОС.
- MSRV не указан, при этом код использует let-chains (Rust 1.88+). В PhoeCord `rust-version = "1.88"` и отдельная MSRV-джоба.
- Имя CA изменено на «PhoeCord local CA (this computer only)». CA, созданный Ashcord, `phoecord untrust` не удалит — для этого остаётся `ashcord untrust`.
