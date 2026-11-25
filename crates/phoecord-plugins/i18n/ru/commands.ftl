# Команда /phoecord. Ответ любой подкоманды `/phoecord <имя>` собирается, пока работает этот
# плагин, поэтому тексты подкоманд других плагинов тоже лежат здесь, с id плагина в начале ключа.
plugins-header = Плагины PhoeCord
plugin-off = Плагин `{ $id }` выключен. Включите его командой `/phoecord toggle { $id }`.
unknown-command = Неизвестная команда `{ $name }`.
toggle-unknown = Плагина `{ $id }` нет. Попробуйте `/phoecord plugins`.
toggle-self = `{ $id }` нельзя выключить отсюда; используйте `phoecord disable { $id }`.
toggle-on = `{ $id }` теперь включён.
toggle-off = `{ $id }` теперь выключен.
toggle-reconnect = Вступит в силу после переподключения Discord (Ctrl+R).
toggle-failed = Не удалось переключить `{ $id }`: { $error }
# fake_deafen
fake_deafen-on = Fake deafen **включён**: остальные видят вас с выключенными микрофоном и звуком.
fake_deafen-off = Fake deafen **выключен**.
fake_deafen-unknown-state = Неизвестное состояние `{ $state }`. Используйте `on` или `off`.
# follow_user
follow_user-following = Слежу за пользователем `{ $id }`.
follow_user-trigger = Захожу в канал отслеживаемого пользователя.
follow_user-unfollowed = Больше ни за кем не слежу.
follow_user-bad-user = `{ $user }` не похоже на id или упоминание пользователя.

# FakeDeafenDetector: `/phoecord flagged`.
fake_deafen_detector-none = Никого не поймали на разговоре при отметке «микрофон выключен» или «звук выключен».
fake_deafen_detector-header = Пойманы на разговоре при отметке «микрофон выключен» или «звук выключен»: { $count }
fake_deafen_detector-line = { $user }: пойман на разговоре { $count } раз при отметке «выключен микрофон/звук» (первый раз: { $first }, последний: { $last }), скорее всего притворяется

# AntiRickroll: `/phoecord rickrolls`.
anti_rickroll-none = Рикроллов в ссылках пока не замечено.
anti_rickroll-header = Сообщения со ссылками на рикролл: { $count }
anti_rickroll-line-known = { $channel } сообщение { $message }: `{ $url }` это известный рикролл.
anti_rickroll-line-custom = { $channel } сообщение { $message }: `{ $url }` подходит под один из ваших фильтров на рикроллы.
