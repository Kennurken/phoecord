# Как вносить изменения

Работаем по GitHub Flow: каждое изменение — отдельная ветка и Pull Request. Как устроен проект — [`ARCHITECTURE.md`](ARCHITECTURE.md), что делаем сейчас — [`ROADMAP.md`](ROADMAP.md).

Префиксы веток: `feat/`, `fix/`, `refactor/`, `chore/`, `docs/` (пример: `feat/proxy-gateway`). Коммиты — [Conventional Commits](https://www.conventionalcommits.org/ru/), область — имя крейта без `phoecord-`: `feat(proxy): wire gateway into plugin chain`, `fix(loader): reject ambiguous AOB matches`.

## Цикл задачи

1. **Ветка от свежего `main`.** После этого `git branch` должен показывать новую ветку с префиксом.

   ```bash
   git checkout main
   git pull origin main
   git checkout -b feat/my-feature
   ```

2. **Разработка и атомарные коммиты.** Один коммит — одно законченное изменение, которое собирается и проходит тесты.

   ```bash
   git add <файлы>
   git commit -m "feat(plugins): port silent_typing to Plugin API v1"
   ```

3. **Синхронизация с `main` перед отправкой.**

   ```bash
   git fetch origin main
   git rebase origin/main
   ```

   При конфликтах: исправьте файлы, `git add <файлы>`, затем `git rebase --continue`. После rebase проверки должны проходить снова.

4. **Пуш и Pull Request.**

   ```bash
   git push -u origin feat/my-feature
   ```

   На GitHub: **Compare & pull request** → заполните шаблон → в **Reviewers** укажите владельцев.

5. **Review и слияние — владельцы.** Они смотрят **Files changed**, оставляют комментарии и нажимают **Approve**, затем **Squash and merge**; ветка удаляется.

## Перед Pull Request

CI запускает те же проверки на Linux, Windows и macOS. Локально:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Правила кода

- **Границы крейтов.** `phoecord-core` не зависит ни от одного нашего крейта и не делает I/O. Плагины не открывают сокеты и файлы — только через `Host`. Загрузчик (`phoecord-loader`) не нужен сетевым плагинам и не должен становиться их зависимостью.
- **Код из Ashcord.** Файл, перенесённый из [Ashcord](https://github.com/Tjabooo/ashcord), начинается с заголовка:

  ```rust
  // Ported from Ashcord (https://github.com/Tjabooo/ashcord), MIT, Copyright (c) 2026 Tjabooo.
  // See LICENSE and the Credits section of README.md.
  ```

  Заголовок не удаляется, даже если файл сильно переписан. Миграция модулей — [`docs/ASHCORD_AUDIT.md`](docs/ASHCORD_AUDIT.md).
- **Plugin API.** Изменение `Event`, `Verdict`, `Host` или `Manifest` — отдельный PR с пометкой `api` и записью в [`CHANGELOG.md`](CHANGELOG.md). Новый вариант — minor, изменение существующего — bump `API_VERSION`.
- **`unsafe`.** Допускается только в `phoecord-loader` и только с комментарием `// SAFETY:` над каждым блоком.
- **Тесты без сети.** Тесты не ходят в Discord и не требуют токена. Для Gateway — записанные payload'ы без личных данных.
- **Плагины.** Каждый плагин — свой `id` в `snake_case`, тесты рядом с кодом, строка в таблице плагинов в [`ROADMAP.md`](ROADMAP.md).

## Чего не присылать

- Токены Discord, `ca.key`, `ca.pem`, дампы трафика с личными данными, бинарники клиента Discord и их дизассемблированные куски.
- Функции для массовых рассылок, самоботов, обхода банов и модерации, сбора чужих данных. Такие PR закрываются без review.

Уязвимости — не в issue, а лично: [`SECURITY.md`](SECURITY.md).
