package org.zohara.agent.update

/**
 * A version: up to four numbers and an optional pre-release (`-beta.1`); a leading `v` and build metadata (`+abc`) are ignored; a pre-release is
 * older than the same version without one. The same rules as `zo_core::update::Semver` on the desktop and the server, so every client agrees.
 */
class AppVersion private constructor(private val nums: LongArray, private val pre: List<String>) : Comparable<AppVersion> {
  val isPrerelease: Boolean get() = pre.isNotEmpty()

  override fun compareTo(other: AppVersion): Int {
    for (i in 0 until 4) if (nums[i] != other.nums[i]) return nums[i].compareTo(other.nums[i])
    if (pre.isEmpty() || other.pre.isEmpty()) return if (pre.isEmpty() == other.pre.isEmpty()) 0 else if (pre.isEmpty()) 1 else -1
    for (i in 0 until minOf(pre.size, other.pre.size)) {
      val a = pre[i]; val b = other.pre[i]
      if (a == b) continue
      val an = a.all { it.isDigit() }; val bn = b.all { it.isDigit() }
      return when {
        an && bn -> a.toBigInteger().compareTo(b.toBigInteger())
        an != bn -> if (an) -1 else 1
        else -> a.compareTo(b)
      }
    }
    return pre.size.compareTo(other.pre.size)
  }

  override fun equals(other: Any?) = other is AppVersion && compareTo(other) == 0
  override fun hashCode() = nums.contentHashCode() * 31 + pre.hashCode()

  companion object {
    private val core = Regex("""[0-9]{1,12}""")
    private val ident = Regex("""[0-9A-Za-z-]+""")

    fun parse(text: String?): AppVersion? {
      var s = text?.trim() ?: return null
      if (s.startsWith("v") || s.startsWith("V")) s = s.substring(1)
      s = s.substringBefore('+')
      val coreText = s.substringBefore('-')
      val preText = if (s.contains('-')) s.substringAfter('-') else null
      val parts = coreText.split('.')
      if (parts.size > 4 || parts.any { !core.matches(it) }) return null
      val nums = LongArray(4)
      parts.forEachIndexed { i, p -> nums[i] = p.toLong() }
      val pre = if (preText == null) emptyList() else preText.split('.').also { ids -> if (ids.any { !ident.matches(it) }) return null }
      return AppVersion(nums, pre)
    }

    /** Is [latest] newer than [current]? A version that cannot be read is never "newer" (no false alarms). */
    fun isNewer(current: String?, latest: String?): Boolean {
      val c = parse(current) ?: return false
      val l = parse(latest) ?: return false
      return l > c
    }
  }
}
