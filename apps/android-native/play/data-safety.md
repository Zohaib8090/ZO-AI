# Data safety form: answers

Source of truth for the facts: `docs/PRIVACY.md` (the privacy notice served at `/privacy`) and the code of the app. These are the answers to type into Play Console > App content > Data safety.
Review them with the owner and again whenever a feature changes what is collected. A wrong form is a policy violation: if in doubt, declare more, not less.

## Overview questions

| Question | Answer |
|---|---|
| Does the app collect or share any of the required user data types? | Yes |
| Is all of the user data collected by the app encrypted in transit? | Yes (HTTPS/TLS only; no cleartext traffic is allowed in the release build) |
| Do you provide a way for people to request that their data is deleted? | Yes: in the app (Settings > Account > Delete my account and all data) and on the web: `https://zohara-agent.duckdns.org/delete-account` |
| Does the app follow the Families policy / is it designed for children? | No. The audience is 13+ (see below) |
| Independent security review (optional badge) | No |

## Data types

"Collected" = leaves the phone to our server. "Shared" = goes to a third party that is not acting on our behalf. The model provider receives the conversation because the person chose it and supplied the key, so it is declared as shared to be on the safe side (the owner may reclassify it after Google's definitions are checked; see the note below the table).

| Category > type | Collected | Shared | Required or optional | Purpose | Notes |
|---|---|---|---|---|---|
| Personal info > Email address | Yes | No | Required | Account management, App functionality | Google or e-mail sign-in (Firebase Authentication) |
| Personal info > User IDs | Yes | No | Required | Account management, App functionality | Firebase user id and a derived anonymous per-user id |
| Personal info > Name | Yes (Google sign-in only) | No | Optional | Account management | Firebase Authentication keeps the display name of a Google account; the app reads it on the phone only to greet the person by first name. `docs/PRIVACY.md` does not list the name yet: add it there before launch so that the notice and this form agree |
| Messages > Other in-app messages | Yes | Yes (the model provider the person chooses) | Required to chat | App functionality | Chats are stored on our server (Firestore, Europe) and sent to the chosen provider to be answered |
| Photos and videos > Photos | Yes | Yes (same provider) | Optional | App functionality | Only pictures the person attaches or takes inside the app |
| Files and docs > Files and docs | Yes | Yes (same provider) | Optional | App functionality | Only files the person attaches (PDF, text, zip, json); a PDF's text is read on the phone |
| App activity > Other user-generated content | Yes | No | Optional | App functionality | Memory notes, personalities, projects, artifacts, scheduled tasks |
| Personal info > Other info | Yes | Yes (the provider it belongs to) | Required to chat | App functionality | The API key the person types in. It is stored encrypted on the server and only used for that person's own requests |
| Device or other IDs | Yes | No | Optional (only with notifications on) | App functionality | The Firebase Cloud Messaging token and a device label, to deliver push messages; removed at sign-out |
| Audio > Voice or sound recordings | No | n/a | n/a | n/a | Dictation uses the phone's own speech recognizer; the app receives only the text. The recognizer is the phone maker's or Google's service, not ours |
| Location, Contacts, Calendar, Health, Financial info, Web browsing, Search history, Installed apps, Crash logs, Diagnostics | No | n/a | n/a | n/a | No analytics, no crash reporting, no advertising SDK in the app |

Not collected and not used: advertising ID, precise or approximate location, contacts.

Note on "shared": Google does not count a transfer to a service provider that processes data on the developer's behalf, but here the person picks the provider and the provider's own policy then applies, so "shared" is the conservative answer. Declaring it does not need any other change.

## Data handling details to enter

* Data is encrypted in transit: yes.
* Data can be deleted: yes. In-app deletion removes everything listed above (chats, memory, files, devices, saved keys, registration). The sign-in record at Firebase Authentication is deleted too. A minimal marker (anonymous account number and time) is kept so that a sign-in left open on another device cannot silently re-create the account; Google removes sign-in data from backups within about 180 days. This is stated at `/delete-account` and in `/privacy`.
* Data retention: until the person deletes it.
* No data is sold. No data is used for advertising or analytics.

## Account deletion (Play policy "Account deletion requirement")

Play requires an in-app path and a public web page that lets people ask for deletion without installing the app.

* In the app: Settings > Account > "Delete my account and all data..." (type DELETE to confirm). The same page has a "How deletion works" button that opens the web page.
* Public URL to enter in the console (App content > Data safety > "Delete account URL"): `https://zohara-agent.duckdns.org/delete-account`. The server serves it (`crates/zo-server`, text in `docs/DELETE-ACCOUNT.md`): what is deleted, what is kept and for how long, and an e-mail route for people who cannot sign in.

## Privacy policy URL

`https://zohara-agent.duckdns.org/privacy`. The server serves it (`/privacy`, text in `docs/PRIVACY.md`). It must be reachable without sign-in, must not be a PDF, and must name the developer: it does ("Operator: Zohaib Baig, contact: ..."). The operator's `ZO_CONTACT_EMAIL` fills the contact line; the server refuses to start without it. Have the text reviewed before launch (it is a template, not legal advice).

## Target audience and Families

Target age: 18 and over is the safest choice (some model providers require 18+; the terms say so). Choose 18+ only in "Target audience and content". Do not enrol in the Families programme.
