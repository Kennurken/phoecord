# Журнал изменений

Все заметные изменения проекта ([Keep a Changelog](https://keepachangelog.com/ru/1.1.0/), [SemVer](https://semver.org/lang/ru/)).

## [Не выпущено]

### Добавлено

- Cargo workspace из 8 крейтов: `core`, `proxy`, `plugin-runtime`, `plugin-sdk`, `plugins`, `loader`, `installer`, `cli`.
- Plugin API v1 (`phoecord-core::plugin`): `Manifest`, `Event`, `EventKind`, `Verdict`, `Host`.
- Цепочка плагинов `phoecord-plugin-runtime::Chain`: первый `Drop`/`Reply` останавливает цепочку.
- AOB-сканер сигнатур в `phoecord-loader` (`??`-маски, отказ при неоднозначном совпадении).
- Документация: README, ARCHITECTURE, ROADMAP, CONTRIBUTING, SECURITY, аудит Ashcord; шаблоны issue и PR; CI на Linux, Windows и macOS.

### Перенесено из Ashcord 0.1.0 (`5ade8b9`)

- `val`, `etf`, маршруты REST → `phoecord-core`.
- `codec`, `ca`, `trust` → `phoecord-proxy`. CA переименован в «PhoeCord local CA».
- Плагин `no_track` → `phoecord-plugins`, на новом Plugin API.

### Исправлено

- `trust.rs`: неиспользуемый импорт `PathBuf` ломал сборку с `-D warnings` на Windows и macOS.
