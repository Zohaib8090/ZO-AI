/* Zohara Agent service worker. Two jobs: keep the app shell (the page and its icons) so the installed app opens at once, and show
   notifications (also when a push arrives while the browser is closed). It never touches /api/: answers, chats and keys are
   not cached, ever. The server replaces __VERSION__ with a hash of the page, so a new page means a new worker and the
   "New version available - Reload" toast; the new worker waits until the person presses Reload. */
const VERSION = "__VERSION__";
const CACHE = "zo-shell-" + VERSION;
const SHELL = ["/", "/manifest.webmanifest", "/icons/icon-192.png", "/icons/icon-512.png", "/icons/maskable-192.png", "/icons/maskable-512.png", "/apple-touch-icon.png"];

self.addEventListener("install", e => {
  e.waitUntil(caches.open(CACHE).then(c => Promise.all(SHELL.map(u =>
    fetch(new Request(u, { cache: "reload" })).then(r => (r.ok && !r.redirected) ? c.put(u, r) : null).catch(() => null)))));
});

self.addEventListener("activate", e => {
  e.waitUntil((async () => {
    for (const k of await caches.keys()) if (k.startsWith("zo-shell-") && k !== CACHE) await caches.delete(k);
    await self.clients.claim();
  })());
});

self.addEventListener("message", e => {
  const d = e.data || {};
  if (d.type === "skip-waiting") self.skipWaiting();
  else if (d.type === "version" && e.source) e.source.postMessage({ type: "version", version: VERSION });
});

/* stale-while-revalidate for the shell only: the cached copy answers at once, a fresh one is fetched for the next time */
self.addEventListener("fetch", e => {
  const r = e.request;
  if (r.method !== "GET") return;
  const u = new URL(r.url);
  if (u.origin !== location.origin || u.pathname.startsWith("/api/")) return;
  const key = u.pathname === "/index.html" ? "/" : u.pathname;
  if (!SHELL.includes(key)) return;
  e.respondWith((async () => {
    const cache = await caches.open(CACHE);
    const hit = await cache.match(key);
    const fresh = fetch(key === "/" ? "/" : r).then(res => { if (res.ok && !res.redirected) cache.put(key, res.clone()); return res; });
    if (hit) { e.waitUntil(fresh.catch(() => {})); return hit; }
    return fresh;
  })());
});

/* a push from the server (only sent when the person's browser was not connected): generic text, the chat's number and, if the
   person allowed it, the chat's title. Nothing is shown when a Zohara Agent window has the focus. */
self.addEventListener("push", e => {
  let d = {};
  try { d = e.data ? e.data.json() : {}; } catch {}
  e.waitUntil((async () => {
    const wins = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
    if (wins.some(w => w.focused)) return;
    const chat = typeof d.chat === "string" ? d.chat : "";
    const title = typeof d.title === "string" && d.title ? d.title : "";
    await self.registration.showNotification(title || "Zohara Agent", {
      body: title ? "Zohara Agent: your answer is ready" : "Your answer is ready",
      tag: "zo-chat-" + chat, icon: "/icons/icon-192.png", data: { chat },
    });
  })());
});

self.addEventListener("notificationclick", e => {
  e.notification.close();
  const chat = (e.notification.data && e.notification.data.chat) || "";
  e.waitUntil((async () => {
    const wins = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
    const w = wins.find(x => new URL(x.url).origin === location.origin);
    if (w) { try { await w.focus(); } catch {} w.postMessage({ type: "zo-open-chat", chat }); return; }
    await self.clients.openWindow(chat ? "/?chat=" + encodeURIComponent(chat) : "/");
  })());
});
