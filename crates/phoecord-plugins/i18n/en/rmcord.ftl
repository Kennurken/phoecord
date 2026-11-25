# rmcord: answers of `/phoecord rmcord` and the progress lines of the script.
rmcord-warning = Warning: automating actions on a user account can break Discord's Terms of Service. Your account can be rate limited, suspended or banned, and deleted messages are gone for good. You use this at your own risk; the authors have no affiliation with Discord and accept no liability.
rmcord-help =
    **rmcord**: delete your own messages in bulk. Pick the control with `action`:
    `start` deletes in `channels` (ids, comma or space separated). Filters: `from` and `to` (YYYY-MM-DDTHH:MM in your local time, or YYYY-MM-DD), `types` (text, image, video, link, embed, sound, file, sticker; default all), `regex` with `mode` (`include` deletes matches, `exclude` deletes the rest).
    `schedule` takes the same options plus `at` and starts then. `cancel` drops the schedule.
    `pause`, `resume` and `stop` control the run; `status` shows the counts, channel and ETA; `log` shows the latest progress lines.
    Messages go one by one: a search page every 10 s, a delete every 0.6 s, slower after Discord rate limits.
rmcord-not-signed-in = Not signed in yet: wait for the gateway to connect, then try again.
rmcord-no-channels = Give at least one channel id in `channels`.
rmcord-already-running = A deletion is already running. Stop it first with `/phoecord rmcord stop`.
rmcord-started = Started deleting your messages in { $count } channel(s). Follow it with `/phoecord rmcord status`.
rmcord-schedule-failed = Could not set the timer: { $error }
rmcord-need-at = Give `at` (YYYY-MM-DDTHH:MM, your local time).
rmcord-schedule-past = That time is already past.
rmcord-scheduled = Scheduled for { $when }.
rmcord-schedule-cancelled = Schedule cancelled.
rmcord-nothing-scheduled = Nothing is scheduled.
rmcord-nothing-running = No deletion is running.
rmcord-paused = Paused.
rmcord-resumed = Resumed.
rmcord-stopped = Stopping: the run ends at its next check.
rmcord-bad-date = Not a valid date: `{ $value }`. Use YYYY-MM-DDTHH:MM (your local time) or YYYY-MM-DD.
rmcord-bad-type = Unknown message type `{ $type }`. Types: text, image, video, link, embed, sound, file, sticker.
rmcord-bad-mode = Unknown mode `{ $mode }`. Use `include` or `exclude`.
rmcord-bad-action = Unknown action `{ $action }`. Use start, schedule, cancel, pause, resume, stop, status, log or help.
rmcord-regex-ignored = The regex is not valid here, so it is ignored and every message passes (as the script treats an invalid regex).
rmcord-sched-busy = The scheduled deletion was skipped: another one is still running.
rmcord-unknown = unknown
rmcord-status-idle = No deletion has run yet.
rmcord-state-running = running
rmcord-state-paused = paused
rmcord-state-stopped = stopped
rmcord-state-done = done
rmcord-status = rmcord is { $state }: deleted { $deleted } of { $total }, failed { $failed }. Search delay { $search } ms, delete delay { $delete } ms.
rmcord-status-channel = Channel: { $channel }
rmcord-status-eta = ETA for this channel: { $eta }
rmcord-status-scheduled = Scheduled for { $when } ({ $count } channel(s)).
rmcord-log-empty = No progress lines yet.
rmcord-log-searching = Searching { $label }...
rmcord-log-not-found = Channel { $id } not found, skipping.
rmcord-log-archived = Thread is archived, skipping message.
rmcord-log-delete-limited = Rate limited, waiting { $wait }s. Delete delay raised to { $delay }ms.
rmcord-log-delete-retry = Delete failed ({ $status }), retrying... ({ $n }/{ $max })
rmcord-log-search-limited = Search rate limited, waiting { $wait }s. Search delay raised to { $delay }ms.
rmcord-log-not-indexed = Channel not indexed yet, waiting...
rmcord-log-search-error = Search error ({ $status }), skipping { $label }.
rmcord-log-index-updating = Search index updating ({ $total } results pending), waiting...
rmcord-log-no-more = No more messages in { $label }.
rmcord-log-skipped = Skipped { $count } filtered/non-deletable messages, next page...
rmcord-log-paused = Paused.
rmcord-log-resumed = Resumed.
rmcord-log-stopped = Stopped by user.
rmcord-log-done = All done!
rmcord-log-deleted = Deleted in { $channel } ({ $when }): { $content }
rmcord-log-failed = Failed in { $channel } ({ $when }): { $content }
