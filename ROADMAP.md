# Роадмап PhoeCord

План от форка Ashcord до релиза v1.0 на Project Phoenix. Как всё устроено — [`ARCHITECTURE.md`](ARCHITECTURE.md). Отмечайте чекбокс в том же PR, где сделана работа.

| Фаза | Когда | Цель | Версии |
|:---|:---|:---|:---|
| **0. Форк** | ноябрь 2026 | Workspace, документация, CI | ✅ |
| **1. До публичной беты Phoenix Canary** | до выхода Canary | Сетевой слой работает на текущем Discord, динамические плагины, прототипы инсталлера и загрузчика | `0.2` – `0.6` |
| **2. Релиз Phoenix Canary** | после выхода Canary | Реверс, сигнатуры, UI-хуки и темы, v1.0 | `0.7` – `1.0` |

Дат у Фазы 2 нет: она начинается, когда Discord выпускает Phoenix Canary. Пока его нет, всё, что можно, делаем на текущем клиенте.

---

## Фаза 0. Форк ✅

- [x] Аудит Ashcord 0.1.0 — [`docs/ASHCORD_AUDIT.md`](docs/ASHCORD_AUDIT.md)
- [x] Cargo workspace: `core`, `proxy`, `plugin-runtime`, `plugin-sdk`, `plugins`, `loader`, `installer`, `cli`
- [x] Перенос `val`, `etf`, маршрутов, `codec`, `ca`, `trust` с сохранением copyright
- [x] Plugin API v1 (`Manifest`, `Event`, `Verdict`, `Host`) и `Chain`
- [x] AOB-сканер с тестами
- [x] README, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, шаблоны issue и PR, CI на 3 ОС

---

## Фаза 1. До публичной беты Phoenix Canary

### 1.1 Рефакторинг ядра: прокси на новом API · `0.2`

- [ ] Перенести `proxy.rs`, `rest.rs`, `gateway.rs` в `phoecord-proxy`, заменить `App::on_*` на `Chain`
- [ ] `Host` для прокси: настройки из `config.toml`, KV в папке данных, `self_id` из READY, `notify` с rate limit
- [ ] `config.toml`: свободные секции `[plugins.<id>]` вместо поля на каждый плагин; live-reload сохранить
- [ ] Перенести `state.rs` (знания об аккаунте) за `Host`, убрать глобальный `Mutex` на всю цепочку
- [ ] CLI: `run`, `launch`, `setup`, `trust`, `untrust`, `plugins`, `enable`, `disable`, `paths`
- [ ] `selftest`: REST + Gateway (JSON/zlib, ETF/zstd) против настоящего Discord без логина
- [ ] Записанные payload'ы Gateway без личных данных для тестов (`tests/fixtures/`)
- [ ] Бенчмарк: задержка кадра Gateway через цепочку ≤ 1 мс p99 на READY 5 МБ

### 1.2 Встроенные плагины на Plugin API · `0.3`

- [x] `no_track`
- [ ] `silent_typing`
- [ ] `unindent`
- [ ] `game_activity_toggle`
- [ ] `relationship_notifier`
- [ ] `fake_nitro` (эмодзи; стикеры и качество стрима — отдельно)
- [ ] `message_logger` (кэш, удалённые и изменённые, `phoecord log`)
- [ ] Новый: `clear_urls` — чистка трекинговых параметров в исходящих сообщениях
- [ ] Новый: слэш-команды PhoeCord (`/phoecord plugins`, `/phoecord toggle`) — локальный ответ на `POST /interactions`
- [ ] `phoecord import-vencord` через поле `vencord` в манифестах

### 1.3 Динамический движок плагинов (WASM) · `0.4`

- [ ] `wit/phoecord.wit`: мир `plugin` (экспорт `on-event`, `wants`, `manifest`) и интерфейс `host`
- [ ] Бэкенд `wasmtime` в `phoecord-plugin-runtime` за тем же `Chain`
- [ ] Лимиты: epoch-таймаут на событие, лимит памяти, отключение плагина при панике
- [ ] Формат пакета: `plugin.toml` + `plugin.wasm`, папка `plugins/` в данных PhoeCord, горячая загрузка
- [ ] Права в манифесте (`events`, `notify`, `kv`): хост не передаёт плагину то, на что он не подписан
- [ ] `phoecord-plugin-sdk`: биндинги + макрос `#[phoecord::plugin]`, пример `examples/hello-plugin`
- [ ] Шаблон плагина на TypeScript через `componentize-js`
- [ ] `clear_urls` собран как WASM-плагин — доказательство, что внешний плагин работает без пересборки PhoeCord

### 1.4 Прототип GUI-инсталлера · `0.5`

- [ ] `phoecord-installer`: поиск Stable / PTB / Canary на Windows, macOS, Linux (Flatpak, Snap)
- [ ] Установка и удаление CA, автозапуск прокси (служба / LaunchAgent / systemd user unit)
- [ ] GUI на GPUI: список клиентов, кнопки «Установить», «Удалить», «Починить» — заодно практика с GPUI перед Фазой 2
- [ ] Полный откат: CA, DLL, автозапуск, папки — одной кнопкой
- [ ] Подписанные релизы и `SHA256SUMS` в GitHub Releases

### 1.5 Прототип DLL sideloading · `0.6`

- [ ] `phoecord-loader` как `version.dll`: форвард всех экспортов на `System32\version.dll` по полному пути
- [ ] `DllMain` без работы под loader lock, рабочий поток, логирование в файл
- [ ] Определение версии клиента (ресурсы exe + хэш `.text`), формат `signatures/*.toml`
- [ ] Inline detour + снятие хуков при выгрузке
- [ ] IPC загрузчик ↔ прокси (named pipe): статус, настройки, события для UI
- [ ] Переустановка после обновления Discord (`app-x.y.z`)
- [ ] Проверка на текущем клиенте: загрузчик стартует и выключается без единого крэша на 3 версиях подряд

### 1.6 Исследование GPUI (Zed)

- [ ] Карта GPUI: `App`, `Window`, элементы, как строится кадр, где живёт палитра темы
- [ ] Тестовое приложение на GPUI, собранное как Phoenix (release, LTO, strip) — эталон для сигнатур
- [ ] Какие функции GPUI переживают LTO и узнаются в бинарнике (строки, паники, `type_name`)
- [ ] Как Zed грузит темы (JSON) и можно ли ожидать того же от Phoenix
- [ ] Отчёт `docs/research/gpui.md` с кандидатами на хуки: палитра, список вкладок настроек, рендер сообщения

**Готовность к Фазе 2:** сетевой слой стабилен на текущем Discord, ≥ 8 плагинов, WASM-плагин ставится без пересборки, загрузчик проверен на текущем клиенте, отчёт по GPUI готов.

---

## Фаза 2. Релиз Phoenix Canary

### 2.1 Разведка Phoenix · `0.7`

- [ ] Слушается ли Phoenix `HTTPS_PROXY` / системного прокси; доверяет ли системному хранилищу; есть ли пиннинг
- [ ] Кодировка и сжатие Gateway в Phoenix (ETF? zstd?), новые события и поля
- [ ] Выбор способа подключения к прокси (ARCHITECTURE.md, «Phoenix и прокси») и его реализация
- [ ] Таблица импортов: `version.dll` или `winhttp.dll` для sideloading; путь установки и схема обновлений
- [ ] `phoecord launch` и инсталлер находят Phoenix

### 2.2 Реверс и AOB-сигнатуры · `0.8`

- [ ] Проект Ghidra (и IDA) на Phoenix Canary: импорт символов GPUI из эталонной сборки (1.6)
- [ ] Сигнатуры: палитра темы, список вкладок настроек, рендер сообщения, создание HTTP-клиента (если нужно для 2.1)
- [ ] Каждая сигнатура уникальна и проверена на двух соседних сборках
- [ ] CI-проверка сигнатур на дампах `.text` (дампы хранятся вне репозитория)
- [ ] Обновление сигнатур без пересборки загрузчика; регламент «новая сборка Canary → сигнатуры за 48 часов»

### 2.3 UI-хуки и движок тем · `0.9`

- [ ] Движок тем: `theme.toml` → палитра GPUI, живое переключение
- [ ] 3 встроенные темы + импорт тем Vencord там, где это переводится в палитру
- [ ] Вкладка «PhoeCord» в настройках клиента: включение плагинов и их настройки
- [ ] `MessageLogger` в UI: удалённые сообщения зачёркнуты, история правок по наведению
- [ ] Деградация проверена: неизвестная сборка → только сеть, без крэшей

### 2.4 Релиз v1.0 · `1.0`

Топ-10 плагинов Vencord по популярности (наша оценка) — все должны работать:

| # | Плагин | Слой | Статус |
|---:|:---|:---|:---|
| 1 | MessageLogger | Сеть + UI | ⏳ |
| 2 | FakeNitro | Сеть | ⏳ |
| 3 | NoTrack | Сеть | ✅ |
| 4 | ClearURLs | Сеть | ⏳ |
| 5 | SilentTyping | Сеть | ⏳ |
| 6 | ShowHiddenChannels | Сеть + UI | ⏳ |
| 7 | Translate | Сеть | ⏳ |
| 8 | PlatformIndicators | Сеть + UI | ⏳ |
| 9 | PermissionsViewer | UI | ⏳ |
| 10 | ViewIcons | UI | ⏳ |

- [ ] Все 10 плагинов выше работают на Phoenix Canary и на текущем клиенте (сетевые)
- [ ] Документация для сторонних разработчиков: `docs/plugins/` — быстрый старт (Rust и TypeScript), справочник событий, права, публикация
- [ ] Стабилизация Plugin API: `API_VERSION = 1` замораживается, изменения — только через minor
- [ ] Инсталлер 1.0 для Windows; macOS и Linux — с сетевым слоем
- [ ] Аудит безопасности CA, прокси и загрузчика перед релизом
- [ ] Релиз в GitHub Releases с подписанными бинарниками и `CHANGELOG.md`

---

## Дальше (после 1.0)

- Каталог сторонних плагинов с проверкой прав и подписей.
- UI-плагины через WASM: декларативные элементы, которые загрузчик превращает в GPUI.
- macOS и Linux для UI-слоя.
- Остальные плагины из [списка Ashcord](https://github.com/Tjabooo/ashcord/blob/main/docs/PLUGINS.md): FixYoutubeEmbeds, MessageLinkEmbeds, ServerInfo, MemberCount и другие.
