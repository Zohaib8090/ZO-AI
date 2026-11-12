# ZO-AI web service: Privacy notice (template)

> **Template.** This text is a starting point for the operator. It is written in plain English to be honest and short. It is not
> legal advice; the operator should have it reviewed for the countries where people use the service before relying on it.
> Last updated: [date]. Operator: Zohaib Baig, contact: [contact e-mail].

This notice covers the public ZO-AI server (the website, the sync from the ZO-AI desktop app, and the `zo` terminal client). It does
not cover the desktop app running alone on your own computer: that keeps everything on your computer.

## Who is responsible

Zohaib Baig ("the operator", [contact e-mail]) runs the service and decides what happens to your data. Ask for access, correction or deletion at that address.

## What is stored

| Data | Why | Where |
|---|---|---|
| Your Google account's identifier and e-mail address, and the sign-in time | to sign you in and keep your data apart from everybody else's | Google Firebase Authentication |
| A random per-user id derived from your account id (the raw id and e-mail are not used in file or database paths) | to name your data | the service |
| Your chats, memory notes, personalities and small settings files | the service's purpose | Firestore, region Europe (eur3), under `users/<your id>` |
| Your API key(s), encrypted | so the assistant can call the model provider you chose | Firestore (encrypted text) and the server's disk while you are active |
| Device tokens (only a hash, never the token), names and last use | to let your desktop app or terminal sync | Firestore |
| Technical logs without message text, keys or tokens (time, route, status) | to run the service and stop abuse | the hosting provider (Render) |

We do not use advertising, analytics or tracking cookies. The browser keeps your sign-in session in its storage because you asked to sign in; nothing else is stored there except your choice that you accepted the notice.

## Where it goes

* **Google (Firebase Authentication and Firestore)**: your sign-in is processed by Google Firebase Authentication, which Google runs **only in the United States**. Chats, memory and settings are stored in Firestore in **Europe (eur3)**. Google acts as our processor and is certified under the EU-US Data Privacy Framework.
* **Render (hosting)**: the server runs at Render, in [region, for example Frankfurt]. Your data passes through its memory and temporary disk.
* **The AI provider you choose** (for example Anthropic, OpenAI, OpenRouter, Google): when you send a message, the server sends the conversation and your own API key to that provider so it can answer. That provider's own terms and privacy policy apply to that data. Most are in the United States.
* **Web search and page fetching**: when the assistant looks something up, the search text or the address of the page leaves the server to the search or web site.

We do not sell data and do not share it with anyone else.

## Who can see it

Nobody except you, in normal use: the service keeps every person's data separate. **The operator can technically access the database and the server**, and holds the secret that encrypts your API key, so the operator could in principle read your chats and decrypt your key. The operator does not do so except to fix a fault, deal with abuse or follow the law. Google and Render, as providers, can technically access their systems too.

## Your API key

You enter your own key. The service uses it only for your own requests. Create a key with a spending limit at your provider and revoke it there if you stop using ZO-AI. You are responsible for the charges on your key.

## How long it is kept

Until you delete it. Account page > Delete account removes your chats, memory, files, settings, device tokens, your local working copy and your registration, and cannot be undone. Google removes sign-in data from its live and backup systems within about 180 days after the account is deleted. Server logs are kept by the hosting provider for a short period. Accounts that are not used for a long time may be removed after notice.

## Your rights

You can **download** everything (Account page > Export: chats, memory and personalities as one JSON file; keys are never included) and **delete** it (see above). You can also ask the operator for access, correction, restriction or objection, withdraw your consent (stop using the service and delete your account), and complain to your data protection authority. Using the service is voluntary; without signing in it cannot work.

## Basis

Providing the service you asked for (contract), your consent when you tick the notice, and the operator's legitimate interest in security and preventing abuse.

## Age

Use the service only if you are old enough to hold a Google Account in your country and, if your country requires it, have the permission of a parent. Some model providers require users to be 18 or older: follow their rules for your key.

## No sensitive data

Do not put passwords, health data, other people's private data or anything you cannot afford to leak into chats or memory. See the Terms.

## Security incidents

If the operator learns that personal data was exposed, it will take the key actions (rotate secrets, tell affected people and the authority where the law requires) as fast as it can.

## Changes

Changes are posted on this page with a new date.
