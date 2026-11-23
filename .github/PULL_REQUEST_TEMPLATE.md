## Что сделано

<!-- Коротко: что и зачем изменено. Название PR — тип(область): описание, например feat(proxy): wire gateway into plugin chain. Если закрывает пункт роадмапа — укажи его: ROADMAP.md, 1.1. -->

## Как проверить

<!-- Шаги для ревьюера: команды, клиент Discord (Stable / Canary / Phoenix), что должно получиться. -->

## Проверки

<!-- Итог локальных проверок или ссылка на зелёный CI. -->

## Чек-лист

- [ ] Ветка с префиксом `feat/` `fix/` `refactor/` `chore/` `docs/`, коммиты в формате Conventional Commits
- [ ] Ветка перебазирована на свежий `main` (`git rebase origin/main`)
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` проходят
- [ ] Тесты не ходят в сеть и не требуют токена Discord
- [ ] Изменение Plugin API (`Event`, `Verdict`, `Host`, `Manifest`) — отмечено меткой `api`, `API_VERSION` поднят, если нужно
- [ ] Новый `unsafe` — только в `phoecord-loader`, с комментарием `// SAFETY:`
- [ ] Код из Ashcord — с заголовком `Ported from Ashcord …`
- [ ] `CHANGELOG.md` (раздел «Не выпущено») и чекбокс в `ROADMAP.md` обновлены
- [ ] Затронуты CA, TLS, хранилища доверия или загрузчик — в Reviewers владельцы (`SECURITY.md`)
- [ ] Нет токенов, `ca.key`, `ca.pem`, дампов трафика и бинарников клиента

<!-- Review и слияние в main делают владельцы. -->
