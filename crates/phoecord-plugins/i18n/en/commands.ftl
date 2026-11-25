# The /phoecord command. Every `/phoecord <name>` answer is rendered while this plugin runs, so
# the answers of other plugins' subcommands live here too, with the plugin id as key prefix.
plugins-header = PhoeCord plugins
plugin-off = The `{ $id }` plugin is off. Switch it on with `/phoecord toggle { $id }`.
unknown-command = Unknown command `{ $name }`.
toggle-unknown = No plugin named `{ $id }`. Try `/phoecord plugins`.
toggle-self = `{ $id }` cannot switch itself off here; use `phoecord disable { $id }`.
toggle-on = `{ $id }` is now on.
toggle-off = `{ $id }` is now off.
toggle-reconnect = Takes effect when Discord reconnects (Ctrl+R).
toggle-failed = Could not switch `{ $id }`: { $error }
# fake_deafen
fake_deafen-on = Fake deafen is **on**: others see you muted and deafened.
fake_deafen-off = Fake deafen is **off**.
fake_deafen-unknown-state = Unknown state `{ $state }`. Use `on` or `off`.
# follow_user
follow_user-following = Following user `{ $id }`.
follow_user-trigger = Joining the followed user's channel.
follow_user-unfollowed = No longer following anyone.
follow_user-bad-user = `{ $user }` is not a user id or mention.

# FakeDeafenDetector: `/phoecord flagged`.
fake_deafen_detector-none = Nobody has been caught speaking while shown as muted or deafened.
fake_deafen_detector-header = Caught speaking while shown as muted or deafened: { $count }
fake_deafen_detector-line = { $user }: caught speaking { $count }x while shown as muted/deafened (first: { $first }, last: { $last }), likely faking it
