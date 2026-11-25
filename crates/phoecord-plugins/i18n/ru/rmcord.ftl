# rmcord: ответы `/phoecord rmcord` и строки прогресса скрипта.
rmcord-warning = Предупреждение: автоматизация действий на пользовательском аккаунте может нарушать Условия использования Discord. Аккаунт могут ограничить по частоте, заблокировать или забанить, а удалённые сообщения не вернуть. Вы используете это на свой риск; авторы не связаны с Discord и не несут ответственности.
rmcord-help =
    **rmcord**: массовое удаление своих сообщений. Действие выбирается в `action`:
    `start` удаляет в каналах из `channels` (id через запятую или пробел). Фильтры: `from` и `to` (ГГГГ-ММ-ДДTЧЧ:ММ по вашему местному времени или ГГГГ-ММ-ДД), `types` (text, image, video, link, embed, sound, file, sticker; по умолчанию все), `regex` с `mode` (`include` удаляет совпавшие, `exclude` удаляет остальные).
    `schedule` принимает те же параметры плюс `at` и стартует в это время. `cancel` отменяет расписание.
    `pause`, `resume` и `stop` управляют запуском; `status` показывает счётчики, канал и оставшееся время; `log` показывает последние строки прогресса.
    Сообщения удаляются по одному: страница поиска раз в 10 с, удаление раз в 0,6 с, медленнее после лимитов Discord.
rmcord-not-signed-in = Вход ещё не выполнен: дождитесь подключения к шлюзу и повторите.
rmcord-no-channels = Укажите хотя бы один id канала в `channels`.
rmcord-already-running = Удаление уже идёт. Сначала остановите его: `/phoecord rmcord stop`.
rmcord-started = Удаление ваших сообщений начато в каналах: { $count }. Следите: `/phoecord rmcord status`.
rmcord-schedule-failed = Не удалось поставить таймер: { $error }
rmcord-need-at = Укажите `at` (ГГГГ-ММ-ДДTЧЧ:ММ, местное время).
rmcord-schedule-past = Это время уже прошло.
rmcord-scheduled = Запланировано на { $when }.
rmcord-schedule-cancelled = Расписание отменено.
rmcord-nothing-scheduled = Ничего не запланировано.
rmcord-nothing-running = Удаление не идёт.
rmcord-paused = Пауза.
rmcord-resumed = Продолжаю.
rmcord-stopped = Останавливаю: запуск закончится на ближайшей проверке.
rmcord-bad-date = Неверная дата: `{ $value }`. Формат ГГГГ-ММ-ДДTЧЧ:ММ (местное время) или ГГГГ-ММ-ДД.
rmcord-bad-type = Неизвестный тип сообщения `{ $type }`. Типы: text, image, video, link, embed, sound, file, sticker.
rmcord-bad-mode = Неизвестный режим `{ $mode }`. Используйте `include` или `exclude`.
rmcord-bad-action = Неизвестное действие `{ $action }`. Доступно: start, schedule, cancel, pause, resume, stop, status, log, help.
rmcord-regex-ignored = Это регулярное выражение здесь недопустимо, поэтому оно игнорируется и проходят все сообщения (так скрипт обходится с неверным regex).
rmcord-sched-busy = Запланированное удаление пропущено: предыдущее ещё идёт.
rmcord-unknown = неизвестно
rmcord-status-idle = Удаление ещё не запускалось.
rmcord-state-running = идёт
rmcord-state-paused = на паузе
rmcord-state-stopped = остановлено
rmcord-state-done = завершено
rmcord-status = rmcord: { $state }; удалено { $deleted } из { $total }, ошибок { $failed }. Задержка поиска { $search } мс, задержка удаления { $delete } мс.
rmcord-status-channel = Канал: { $channel }
rmcord-status-eta = Осталось по этому каналу: { $eta }
rmcord-status-scheduled = Запланировано на { $when } (каналов: { $count }).
rmcord-log-empty = Строк прогресса пока нет.
rmcord-log-searching = Поиск в { $label }...
rmcord-log-not-found = Канал { $id } не найден, пропускаю.
rmcord-log-archived = Ветка в архиве, сообщение пропущено.
rmcord-log-delete-limited = Лимит запросов, жду { $wait } с. Задержка удаления поднята до { $delay } мс.
rmcord-log-delete-retry = Не удалось удалить ({ $status }), повторяю... ({ $n }/{ $max })
rmcord-log-search-limited = Лимит запросов поиска, жду { $wait } с. Задержка поиска поднята до { $delay } мс.
rmcord-log-not-indexed = Канал ещё не проиндексирован, жду...
rmcord-log-search-error = Ошибка поиска ({ $status }), пропускаю { $label }.
rmcord-log-index-updating = Индекс поиска обновляется (ожидает результатов: { $total }), жду...
rmcord-log-no-more = В { $label } больше нет сообщений.
rmcord-log-skipped = Пропущено сообщений, не подошедших под фильтры или не удаляемых: { $count }, следующая страница...
rmcord-log-paused = Пауза.
rmcord-log-resumed = Продолжаю.
rmcord-log-stopped = Остановлено пользователем.
rmcord-log-done = Готово!
rmcord-log-deleted = Удалено в { $channel } ({ $when }): { $content }
rmcord-log-failed = Не удалось в { $channel } ({ $when }): { $content }
