<p align="center">
  <img src="assets/logo.svg" alt="PhoeCord" width="72" height="72" />
</p>

<h1 align="center">PhoeCord</h1>

<p align="center">
  <strong>Нативный модлоадер нового поколения для Discord.</strong><br />
  Плагины в духе Vencord для клиента Project Phoenix (Rust + GPUI) — и для сегодняшнего Discord тоже.
</p>

<p align="center">
  <a href="https://github.com/nanda070/phoecord/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/nanda070/phoecord/ci.yml?branch=main&style=for-the-badge&label=CI" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/Лицензия-MIT-3da639?style=for-the-badge" alt="Лицензия MIT" /></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Rust-2024-dea584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust 2024" /></a>
  <a href="ROADMAP.md"><img src="https://img.shields.io/badge/Статус-pre--alpha-ff8a1f?style=for-the-badge" alt="Статус: pre-alpha" /></a>
</p>

<p align="center">
  <a href="ARCHITECTURE.md">Архитектура</a> ·
  <a href="ROADMAP.md">Роадмап</a> ·
  <a href="docs/ASHCORD_AUDIT.md">Аудит Ashcord</a> ·
  <a href="CONTRIBUTING.md">Как помочь</a> ·
  <a href="SECURITY.md">Безопасность</a> ·
  <a href="CHANGELOG.md">Журнал изменений</a>
</p>

---

## Зачем PhoeCord

Vencord патчит JavaScript веб-клиента Discord. Project Phoenix — это нативный бинарник на Rust и GPUI: JavaScript, который можно патчить, там нет, а стабильного ABI у Rust тоже нет — каждая сборка клиента раскладывает код по-новому. PhoeCord решает это **гибридной архитектурой**: всё, что можно сделать с данными, делается в сети и не зависит от сборки клиента, а в сам клиент мы лезем точечно и только ради интерфейса.

| | |
|:---|:---|
| **Сетевой слой** | Локальный MITM-прокси на Rust между клиентом и Discord: REST и Gateway (JSON/ETF, zlib/zstd). `NoTrack`, `SilentTyping`, `MessageLogger`, `FakeNitro`, `ClearURLs`, свои слэш-команды. Не ломается от патчей клиента |
| **UI-слой** | Нативный загрузчик: прокси-DLL (`version.dll` / `winhttp.dll`), AOB-сканер, хуки в GPUI. Темы (подмена палитры), вкладка настроек в клиенте, показ удалённых сообщений |
| **Плагины** | Единый Plugin API и внешний рантайм (WASM): сторонние плагины ставятся без перекомпиляции и работают в песочнице |
| **Безопасность** | Свой CA на каждой установке, создаётся локально, ограничен name constraint до доменов Discord, никуда не отправляется. Прокси слушает только `127.0.0.1` |
| **Совместимость** | Работает с текущим Discord уже сейчас (сетевой слой), Phoenix — по мере выхода Canary |

> **Статус: pre-alpha.** Репозиторий — форк [Ashcord](https://github.com/Tjabooo/ashcord) в процессе пересборки в Cargo workspace. Сегодня собираются ядро, кодеки Gateway, локальный CA, цепочка плагинов и AOB-сканер; прокси ещё не подключён к новому Plugin API. Модификация клиента нарушает Условия использования Discord, как и Vencord. Используйте на свой риск.

---

## Статус разработки

| Компонент | Крейт | Состояние |
|:---|:---|:---|
| Модель данных (JSON/ETF), Plugin API v1 | `phoecord-core` | ✅ портировано и спроектировано |
| Кодеки Gateway, локальный CA, доверие сертификату | `phoecord-proxy` | ✅ портировано · 🔄 прокси, REST и Gateway — в работе |
| Цепочка плагинов | `phoecord-plugin-runtime` | ✅ нативная · ⏳ WASM-бэкенд |
| Встроенные плагины | `phoecord-plugins` | ✅ `no_track` · 🔄 остальные 6 из Ashcord |
| SDK для сторонних плагинов | `phoecord-plugin-sdk` | ⏳ WIT-контракт и макрос |
| Загрузчик, AOB-сканер, хуки | `phoecord-loader` | ✅ AOB-сканер · ⏳ прокси-DLL, хуки |
| Инсталлер | `phoecord-installer` | ⏳ |
| CLI `phoecord` | `phoecord-cli` | 🔄 каркас |

Этапы и чеклисты — в [`ROADMAP.md`](ROADMAP.md).

---

## Как это работает

```text
                    ┌──────────── phoecord-loader (UI-слой, точечно) ───────────┐
                    │ version.dll → AOB-сигнатуры → хуки GPUI: темы, настройки  │
                    └──────────────────────────────┬────────────────────────────┘
                                                   │ в процессе клиента
┌──────────────────┐   CONNECT     ┌───────────────▼─────────┐   TLS    ┌──────────────┐
│ Discord / Phoenix│ ────────────► │ phoecord-proxy          │ ───────► │ discord.com  │
│ (клиент)         │ ◄──────────── │ REST + Gateway → плагины│ ◄─────── │ gateway      │
└──────────────────┘ 127.0.0.1     └───────────────┬─────────┘          └──────────────┘
                                                   │ Event / Verdict
                                   ┌───────────────▼─────────┐
                                   │ phoecord-plugin-runtime │ нативные + WASM-плагины
                                   └─────────────────────────┘
```

1. Клиент ходит в сеть через PhoeCord как через HTTPS-прокси.
2. Для API и Gateway Discord прокси завершает TLS сам, сертификатом от **локального** CA, и открывает своё TLS-соединение к настоящему Discord. CDN, медиа, голос и все остальные сайты идут прозрачным туннелем.
3. REST-запросы и события Gateway декодируются в один тип `Val` и проходят цепочку плагинов: каждый возвращает `Pass`, `Changed`, `Drop` или `Reply`.
4. Загрузчик в процессе клиента нужен только UI-плагинам. Если после обновления Discord сигнатура не нашлась, UI-слой выключается, а сеть продолжает работать.

Подробно — [`ARCHITECTURE.md`](ARCHITECTURE.md).

---

## Структура репозитория

| Путь | Назначение |
|:---|:---|
| `crates/phoecord-core/` | `Val` (JSON/ETF), Plugin API: `Manifest`, `Event`, `Verdict`, `Host` |
| `crates/phoecord-proxy/` | MITM-прокси, кодеки Gateway (zlib/zstd), локальный CA, хранилища доверия |
| `crates/phoecord-plugin-runtime/` | Цепочка плагинов, позже — WASM-рантайм (wasmtime) |
| `crates/phoecord-plugin-sdk/` | SDK для авторов сторонних плагинов |
| `crates/phoecord-plugins/` | Встроенные плагины, портированные из Ashcord |
| `crates/phoecord-loader/` | Прокси-DLL, AOB-сканер, хуки GPUI (`cdylib`) |
| `crates/phoecord-installer/` | Поиск клиентов, установка CA и загрузчика, откат |
| `crates/phoecord-cli/` | Бинарник `phoecord` |
| `docs/` | [`ASHCORD_AUDIT.md`](docs/ASHCORD_AUDIT.md) — что взято из Ashcord и куда переехало |
| `.github/` | Шаблоны issue и PR, CI (`fmt`, `clippy`, `test` на Linux, Windows, macOS) |

---

## Локальная разработка

Нужен Rust **1.88+** (edition 2024). Для Linux при настройке доверия — `certutil` из NSS (Arch: `nss`, Debian/Ubuntu: `libnss3-tools`, Fedora: `nss-tools`).

```bash
git clone https://github.com/nanda070/phoecord
cd phoecord
cargo build
cargo run            # бинарник phoecord (пока каркас)
```

Те же проверки, что в CI:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Тесты не ходят в сеть и не требуют аккаунта Discord. Новый код из Ashcord помечается заголовком `Ported from Ashcord …` — см. [`CONTRIBUTING.md`](CONTRIBUTING.md).

---

## Порядок работы

- Изменения — только через ветку и Pull Request: префиксы `feat/`, `fix/`, `refactor/`, `chore/`, `docs/`, коммиты в формате `тип(область): описание`, область — имя крейта без `phoecord-` (`feat(proxy): …`). Подробно — [`CONTRIBUTING.md`](CONTRIBUTING.md).
- Готово = CI зелёный, документация и [`CHANGELOG.md`](CHANGELOG.md) обновлены, чекбокс в [`ROADMAP.md`](ROADMAP.md) отмечен.
- Всё, что трогает TLS, CA, хранилища доверия и загрузчик, проходит review владельцев — [`SECURITY.md`](SECURITY.md).
- Никогда не коммитить токены Discord, `ca.key`, дампы трафика и бинарники клиента.

---

## Команда

| Кто | Роль | Discord | Telegram |
|:---|:---|:---|:---|
| **Nanda** | Руководитель проекта | `nandak070` | [@nanda070](https://t.me/nanda070) |

Документы проекта: [лицензия](LICENSE) (MIT) · [как помочь](CONTRIBUTING.md) · [поведение](CODE_OF_CONDUCT.md) · [безопасность](SECURITY.md) · [архитектура](ARCHITECTURE.md) · [роадмап](ROADMAP.md) · [журнал изменений](CHANGELOG.md)

---

## Благодарности

- **[Ashcord](https://github.com/Tjabooo/ashcord)** от **Tjabooo** — основа PhoeCord: MITM-прокси, кодеки Gateway (JSON/ETF, zlib/zstd), локальный CA с name constraint и первые плагины. Распространяется по лицензии MIT, Copyright (c) 2026 Tjabooo; исходный copyright сохранён в [`LICENSE`](LICENSE), портированные файлы помечены заголовком.
- **[Vencord](https://github.com/Vendicated/Vencord)** — идеи и названия плагинов. Код Vencord в PhoeCord не используется.
- **[Zed / GPUI](https://github.com/zed-industries/zed)** — открытые исходники GPUI, по которым мы изучаем UI-слой Phoenix.

PhoeCord не связан с Discord Inc., Vencord и Zed Industries.

---

## Ссылки

| | |
|:---|:---|
| **Репозиторий** | [github.com/nanda070/phoecord](https://github.com/nanda070/phoecord) |
| **Issues** | [баг](https://github.com/nanda070/phoecord/issues/new?template=bug_report.yml) · [фича](https://github.com/nanda070/phoecord/issues/new?template=feature_request.yml) · [плагин](https://github.com/nanda070/phoecord/issues/new?template=plugin_request.yml) |
| **Уязвимости** | только лично, см. [`SECURITY.md`](SECURITY.md) |
| **Основа** | [Tjabooo/ashcord](https://github.com/Tjabooo/ashcord) |

---

<p align="center">
  <sub>© 2026 Cheterin Group · MIT · форк Ashcord · не связан с Discord Inc.</sub>
</p>
