# Zohara Agent: delete your account and your data

> Last updated: {{version}}. Operator: Zohaib Baig, contact: {{contact}}.

This page explains how to delete your Zohara Agent account and everything stored with it. It is the public deletion page for the Zohara Agent
Android app (package `org.zohara.agent`) and for the web version. You can delete your account yourself in a few steps, and you can ask the operator to do it
for you if you can no longer sign in.

## Delete it yourself (about one minute)

* Open Zohara Agent: the Android app, or the web version in a browser, and sign in.
* Open **Settings**, then **Account**.
* Choose **Delete my account and all data**, read the warning and confirm. The Android app and the web version ask you to confirm before anything is removed.

Deleting cannot be undone. If you want a copy first, choose **Export my data** on the same page (one JSON file with your chats, memory and personalities).

## What is deleted

* every chat, memory note, personality and small settings file;
* every file and attachment kept for you on the server;
* your saved API keys (they were stored encrypted);
* every signed-in device (the desktop app, the terminal client, the phone), their sign-in credentials and any waiting sign-in request;
* your working copy on the server and your registration;
* your sign-in record at Google Firebase Authentication. If that last step fails, the page tells you, and you can ask the operator to finish it.

## What is kept, and for how long

* A minimal marker (your anonymous account number and the time of deletion) is kept so that a sign-in left open on another device cannot silently create the account again.
* Google removes sign-in data from its live and backup systems within about 180 days after it is deleted.
* Technical logs without message text, keys or tokens (time, route, status) stay with the hosting provider for a short period.
* Messages you already sent to the AI provider you chose are covered by that provider's own policy; delete them with that provider if you want them gone. Revoke the API key there as well.
* The Zohara Agent desktop app keeps its own data on your computer. Uninstall it or delete its folders (`~/.config/zohara-agent`, `~/.local/share/zohara-agent`) to remove that.

## You cannot sign in

Write to {{contact}} from the e-mail address of the account, with the subject "Delete my Zohara Agent account". The operator checks that the request comes from that address
and deletes the account, normally within 30 days. An account the operator has blocked can still use Export and Delete.

## More

The privacy notice is at /privacy and the terms are at /terms on the same address.
