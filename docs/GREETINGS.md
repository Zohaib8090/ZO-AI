# Home greetings

The line above the composer on the home screen ("Good morning, Zohaib", "Still up?", "Back at it", ...) is no longer fixed text. It is one
message from a table of 20, chosen by the time of day, the weekday and how long the person has been away. Desktop, web and Android show
the same wording.

## One source of truth

`crates/zo-core/data/greetings.json`. Edit only this file, then:

| Client | How it gets the table | Guard against drift |
|---|---|---|
| Desktop (`crates/zohara-agent`) | `zo_core::greetings` embeds it with `include_str!` | none needed |
| Web (`crates/zo-server/web/index.html`) | the server replaces `null /*GREETINGS_JSON*/` in the page with the file (`pwa.rs`); it is also part of the service worker version hash, so a wording change shows "Update available" | `pwa::tests::the_page_gets_the_shared_greetings_inline` |
| Android (`apps/android-native`) | checked-in copy `app/src/main/assets/greetings.json` | `GreetingsTest.theBundledCopyEqualsTheSharedSource` fails with the `cp` command to run |

After editing the JSON: `cp crates/zo-core/data/greetings.json apps/android-native/app/src/main/assets/greetings.json`.

## File format

`messages[]`: `id` (unique), `text` (plain English, at most `max_len` = 48 characters, must be unique), and three tag lists. An empty list means "any".

* `time`: `early` 5-8, `morning` 9-11, `noon` 12-13, `afternoon` 14-17, `evening` 18-20, `night` 21-23, `late` 0-4 (local time).
* `weekday`: `monday`, `friday`, `weekend` (Saturday and Sunday).
* `gap`: `first` (first launch ever), `away` (14 or more days since the last launch), `back` (3-13 days), `streak` (the 5th day in a row or later).
  The numbers are `away_days`, `back_days`, `streak_days` in the file.

`{name}` in a text is the person's first name. It is always written `, {name}` (comma before it) so that without a name the comma and the
placeholder simply disappear: "Good morning, {name}" becomes "Good morning". The name is the first word of the account display name,
else the part of the e-mail before the @ (desktop: the OS real name, else the user name), capitalised. Fewer than half of the messages
use it, and every slot has messages without it.

`golden[]`: fixed cases `{hour, weekday, gap, ids}` listing the candidate ids in table order. All three clients check them, so a rule that
drifts in one client fails a test there.

## Picking

1. The gap of this launch: the day number of the last launch (local calendar day) is stored. Same day: no gap (a second launch the same
   day does not repeat a welcome). No stored day: `first`. Otherwise by the days away, then `streak` when the run of consecutive days
   reaches `streak_days`.
2. Candidates: if the gap is not "none" and some message carries it, only those messages. Otherwise the messages without a gap tag whose
   time and weekday fit.
3. Pick at random among the candidates, weight `1 + 2 (if it has a weekday tag) + 1 (if it has a time tag)`, so "Monday" and "Friday
   evening" show up often on those days but not always. The message of the last launch is removed first (unless it is the only candidate).
4. The choice is made once per launch (desktop: per process, web: per page load, Android: per process), so the home page does not change
   while the app is open, and it is stored as `last_id`.

Clock and random number are arguments of the pure functions (`greet`, `visit`, `candidates`, `pick`), so tests are deterministic.

## Where the state is kept

* Desktop: `<config dir>/greeting.json` (`{last_day, streak, last_id}`), next to the other small settings files.
* Web: `localStorage["zo-greeting"]`, same fields. Private mode just means "first launch" every time.
* Android: SharedPreferences `zo_greeting` (`last_day`, `streak`, `last_id`).

Nothing leaves the device. "Long time since the last chat" is measured as time since the last launch of the app, which is what the
three clients can all see without reading chat history.

## Tests

* `cargo test -p zo-core greetings` (table shape, 2+ candidates for every hour x weekday, no duplicates, length, name substitution, never
  the same twice in a row over 400 launches, gap rules, golden cases).
* `cargo test -p zo-server pwa` (inlining).
* `./gradlew :app:testDebugUnitTest --tests '*GreetingsTest'` (the same list, plus the copy equals the source).
* The web functions are plain JS with the same names as the Rust ones (`greetCandidates`, `greetPick`, `greetVisit`, `greetRender`); there is no JS test runner in the repo.

## Not included on purpose

No religion- or culture-specific dates, no New Year's Day, no birthdays: none can be computed reliably for everyone.
