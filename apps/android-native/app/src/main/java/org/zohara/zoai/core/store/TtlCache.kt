package org.zohara.zoai.core.store

/** A small in-memory cache whose entries expire after [ttlMs] (30 s by default: providers, profiles, status). Not for secrets. */
class TtlCache<K : Any, V : Any>(private val ttlMs: Long = 30_000, private val clock: () -> Long = { System.currentTimeMillis() }) {
  private class Entry<V>(val value: V, val at: Long)

  private val map = LinkedHashMap<K, Entry<V>>()

  @Synchronized
  fun get(key: K): V? {
    val e = map[key] ?: return null
    if (clock() - e.at >= ttlMs) { map.remove(key); return null }
    return e.value
  }

  @Synchronized
  fun put(key: K, value: V) { map[key] = Entry(value, clock()) }

  @Synchronized
  fun invalidate(key: K) { map.remove(key) }

  @Synchronized
  fun clear() { map.clear() }

  /** The cached value, or [load]'s result (cached when it succeeds). */
  suspend fun getOrLoad(key: K, force: Boolean = false, load: suspend () -> V): V {
    if (!force) get(key)?.let { return it }
    return load().also { put(key, it) }
  }
}
