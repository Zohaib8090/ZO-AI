# Zohara Agent web service: Privacy notice

> Last updated: {{version}}. Operator: Zohaib Baig, contact: {{contact}}.

This notice covers the public Zohara Agent server (the website, the sync from the Zohara Agent desktop app, and the `zo` terminal client). It does
not cover the desktop app running alone on your own computer: that keeps everything on your computer.

## Who is responsible

Zohaib Baig ("the operator", {{contact}}) runs the service and decides what happens to your data. Ask for access, correction or deletion at that address.

## What is stored

| Data | Why | Where |
|---|---|---|
| Your Google account's identifier and e-mail address, or, if you sign up with e-mail and password, your e-mail address and a **hash of your password** (never the password itself), and the sign-in time | to sign you in and keep your data apart from everybody else's | Google Firebase Authentication (Google's servers in the United States) |
| A random per-user id derived from your account id (the raw id and e-mail are not used in file or database paths) | to name your data | the service |
| Your chats, memory notes, personalities and small settings files | the service's purpose | Firestore, region Europe (eur3), under `users/<your id>` |
| Your API key(s), encrypted | so the assistant can call the model provider you chose | Firestore (encrypted text) and the server's disk while you are active |
| Your signed-in devices: a name (for example "Zohara Agent desktop on <computer name>"), the kind, when it was created and last used, and only a hash of its sign-in credential (never the credential) | to let your desktop app or terminal sync, and so you can see and sign out devices (Account page > signed-in devices) | Firestore |
| A waiting sign-in request from your desktop app or terminal (device name, kind, the network address it came from, a short code, for at most 10 minutes) | so you can approve it in the browser | the server's memory only |
| Technical logs without message text, keys or tokens (time, route, status) | to run the service and stop abuse | the hosting provider (Render) |

**The desktop app sends nothing to this service until you sign in.** Without signing in, the Zohara Agent desktop app keeps everything on your computer and works fully; signing in is optional and only turns on sync with the web version. The credential that keeps a signed-in device connected is stored in that device's key store, not in a file. The web version needs a sign-in.

We do not use advertising, analytics or tracking cookies. The browser keeps your sign-in session in its storage because you asked to sign in; nothing else is stored there except your choice that you accepted the notice.

## Where it goes

* **Google (Firebase Authentication and Firestore)**: your sign-in is processed by Google Firebase Authentication, which Google runs **only in the United States**. Chats, memory and settings are stored in Firestore in **Europe (eur3)**. Google acts as our processor and is certified under the EU-US Data Privacy Framework.
* **Render (hosting)**: the server runs at Render, in {{region}}. Your data passes through its memory and temporary disk.
* **The AI provider you choose** (for example Anthropic, OpenAI, OpenRouter, Google): when you send a message, the server sends the conversation and your own API key to that provider so it can answer. That provider's own terms and privacy policy apply to that data. Most are in the United States.
* **Web search and page fetching**: when the assistant looks something up, the search text or the address of the page leaves the server to the search or web site.
* **Voice input (optional)**: if you press the microphone button, your browser's own speech recognition listens. In Chrome and Edge the audio is sent to the browser maker's speech service (Google or Microsoft), not to our server; Safari uses Apple's. Our server only ever receives the text you send. The browser asks for your permission first, and you can withdraw it in the browser's site settings. "Read answers aloud" uses the voices of your browser or system; some browsers use an online voice for that, and then the answer text goes to the browser maker.

We do not sell data and do not share it with anyone else.

## Who can see it

Nobody except you, in normal use: the service keeps every person's data separate. **The operator can technically access the database and the server**, and holds the secret that encrypts your API key, so the operator could in principle read your chats and decrypt your key. The operator does not do so except to fix a fault, deal with abuse or follow the law. Google and Render, as providers, can technically access their systems too.

## Your API key

You enter your own key. The service uses it only for your own requests. Create a key with a spending limit at your provider and revoke it there if you stop using Zohara Agent. You are responsible for the charges on your key.

## How long it is kept

Until you delete it. Account page > Delete account removes your chats, memory, files, settings, signed-in devices, your local working copy and your registration, and cannot be undone. Deleting also removes your sign-in record at Google Firebase Authentication; if that step fails, the page tells you so and you can ask the operator to finish it. Google removes sign-in data from its live and backup systems within about 180 days after it is deleted. After a deletion a minimal marker (your anonymous account number and the time) is kept, so that a sign-in left open on another device cannot silently create the account again. Server logs are kept by the hosting provider for a short period. Accounts that are not used for a long time may be removed after notice.

## Your rights

You can **download** everything (Account page > Export: chats, memory and personalities as one JSON file; keys are never included) and **delete** it (see above). You can also ask the operator for access, correction, restriction or objection, withdraw your consent (stop using the service and delete your account), and complain to your data protection authority. Using the service is voluntary; the web version cannot work without signing in, while the desktop app works without it.

## Basis

Providing the service you asked for (contract), your consent when you tick the notice, and the operator's legitimate interest in security and preventing abuse. When you tick, the server records which version of this notice and the terms you accepted and when (one small record with your anonymous account number, kept until you delete your account). An account that the operator has blocked can still use Export and Delete account.

## Age

Use the service only if you are old enough to hold a Google Account in your country and, if your country requires it, have the permission of a parent. Some model providers require users to be 18 or older: follow their rules for your key.

## No sensitive data

Do not put passwords, health data, other people's private data or anything you cannot afford to leak into chats or memory. See the Terms.

## Security incidents

If the operator learns that personal data was exposed, it will take the key actions (rotate secrets, tell affected people and the authority where the law requires) as fast as it can.

## Changes

Changes are posted on this page with a new date.
