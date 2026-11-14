package org.zohara.agent.core.net

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SendEventParserTest {
  private fun p(s: String) = SendEventParser.parse(s)

  @Test fun everyEvent() {
    assertEquals(SendEvent.Stream(7, null, null), p("""{"event":"stream","id":7}"""))
    assertEquals(SendEvent.Stream(8, "desktop", "dev1"), p("""{"event":"stream","id":8,"target":"desktop","device":"dev1"}"""))
    assertEquals(SendEvent.Chat("c1", "Hello"), p("""{"event":"chat","id":"c1","title":"Hello"}"""))
    assertEquals(SendEvent.Delta("Hi"), p("""{"event":"delta","text":"Hi"}"""))
    assertEquals(SendEvent.Text("Hi there"), p("""{"event":"text","text":"Hi there"}"""))
    assertEquals(SendEvent.Steps(7), p("""{"event":"steps","n":7}"""))
    assertEquals(SendEvent.Stuck(8, "read_file"), p("""{"event":"stuck","n":8,"tool":"read_file"}"""))
    assertEquals(SendEvent.Steps(0), p("""{"event":"steps"}"""))
    assertEquals(SendEvent.ToolStart("bash", "ls"), p("""{"event":"tool_start","name":"bash","summary":"ls"}"""))
    assertEquals(SendEvent.ToolDone("bash", true, "a\nb"), p("""{"event":"tool_done","name":"bash","ok":true,"output":"a\nb"}"""))
    assertEquals(SendEvent.Approval(3, "bash", "rm x", false), p("""{"event":"approval","id":3,"tool":"bash","summary":"rm x"}"""))
    assertEquals(SendEvent.Usage(10, 20), p("""{"event":"usage","input":10,"output":20}"""))
    assertEquals(SendEvent.Stopped, p("""{"event":"stopped"}"""))
    assertEquals(SendEvent.Done, p("""{"event":"done"}"""))
    assertEquals(SendEvent.Ping, p("""{"event":"ping"}"""))
  }

  @Test fun questionCard() {
    val ev = p("""{"event":"question","id":4,"questions":[{"question":"Which?","header":"Pick","multi_select":true,"options":[{"label":"A","description":"first"},{"label":"B"}]}]}""")
    ev as SendEvent.Question
    assertEquals(4L, ev.id)
    assertEquals("Which?", ev.questions[0].question)
    assertTrue(ev.questions[0].multiSelect)
    assertEquals(listOf("A", "B"), ev.questions[0].options.map { it.label })
    assertEquals("first", ev.questions[0].options[0].description)
  }

  @Test fun errorUsesMessageOrFriendlyCode() {
    assertEquals(SendEvent.Error("x", "Boom"), p("""{"event":"error","error":"x","message":"Boom"}"""))
    val e = p("""{"event":"error","error":"desktop_offline"}""") as SendEvent.Error
    assertEquals("desktop_offline", e.code)
    assertTrue(e.message.contains("not reachable"))
  }

  @Test fun badLinesAreIgnored() {
    assertNull(p(""))
    assertNull(p("   "))
    assertNull(p("not json"))
    assertNull(p("[1,2]"))
    assertNull(p("""{"no_event":1}"""))
    assertEquals(SendEvent.Unknown("future"), p("""{"event":"future","a":1}"""))
  }

  @Test fun unicodeAndEscapes() {
    assertEquals(SendEvent.Delta("नमस्ते 🌍 \"q\" \\ é"), p("""{"event":"delta","text":"नमस्ते 🌍 \"q\" \\ é"}"""))
  }

  // ---- the line buffer ----
  @Test fun partialLinesAcrossChunks() {
    val b = NdjsonLineBuffer()
    assertEquals(emptyList<String>(), b.feed("""{"event":"del""".toByteArray()))
    assertEquals(listOf("""{"event":"delta","text":"a"}"""), b.feed("""ta","text":"a"}""".toByteArray() + "\n{\"ev".toByteArray()))
    assertEquals(listOf("""{"event":"done"}"""), b.feed("""ent":"done"}""".toByteArray() + "\n".toByteArray()))
  }

  @Test fun multibyteCharacterCutInTwo() {
    val line = """{"event":"delta","text":"日本語🌍"}""" + "\n"
    val bytes = line.toByteArray()
    for (cut in 1 until bytes.size) {
      val b = NdjsonLineBuffer()
      val out = b.feed(bytes.copyOfRange(0, cut)) + b.feed(bytes.copyOfRange(cut, bytes.size))
      assertEquals("cut at $cut", 1, out.size)
      assertEquals(SendEvent.Delta("日本語🌍"), SendEventParser.parse(out[0]))
    }
  }

  @Test fun manyLinesInOneChunkAndCrlf() {
    val b = NdjsonLineBuffer()
    val out = b.feed("{\"event\":\"ping\"}\r\n\n{\"event\":\"done\"}\n".toByteArray())
    assertEquals(listOf("""{"event":"ping"}""", """{"event":"done"}"""), out)
  }

  @Test fun largeLine() {
    val big = "x".repeat(3 * 1024 * 1024)
    val line = """{"event":"tool_done","name":"n","ok":true,"output":"$big"}"""
    val b = NdjsonLineBuffer()
    val bytes = (line + "\n").toByteArray()
    val out = ArrayList<String>()
    var i = 0
    while (i < bytes.size) { val n = minOf(8192, bytes.size - i); out += b.feed(bytes.copyOfRange(i, i + n)); i += n }
    assertEquals(1, out.size)
    assertEquals(big.length, (SendEventParser.parse(out[0]) as SendEvent.ToolDone).output.length)
  }

  @Test fun lastLineWithoutNewlineComesFromFinish() {
    val b = NdjsonLineBuffer()
    assertEquals(emptyList<String>(), b.feed("""{"event":"done"}""".toByteArray()))
    assertEquals(listOf("""{"event":"done"}"""), b.finish())
  }
}
