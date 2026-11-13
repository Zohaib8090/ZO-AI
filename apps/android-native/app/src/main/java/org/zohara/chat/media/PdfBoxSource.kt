package org.zohara.chat.media

import android.content.Context
import com.tom_roush.pdfbox.android.PDFBoxResourceLoader
import com.tom_roush.pdfbox.io.MemoryUsageSetting
import com.tom_roush.pdfbox.pdmodel.PDDocument
import com.tom_roush.pdfbox.pdmodel.encryption.InvalidPasswordException
import com.tom_roush.pdfbox.text.PDFTextStripper
import java.io.File

/**
 * PDF text on the device with PdfBox-Android (Apache 2.0, maintained fork of Apache PDFBox; about 5 MB in the app). `PdfRenderer` of Android only draws pages,
 * it cannot give text. Big PDFs are read with temporary files (not heap), one page at a time.
 */
class PdfBoxSource private constructor(private val doc: PDDocument) : PdfTextSource {
  override val pages: Int = doc.numberOfPages
  private val stripper = PDFTextStripper().apply { sortByPosition = true }

  override fun pageText(page: Int): String {
    stripper.startPage = page
    stripper.endPage = page
    return stripper.getText(doc)
  }

  override fun close() { runCatching { doc.close() } }

  companion object {
    @Volatile private var inited = false

    fun open(context: Context, file: File): PdfTextSource {
      if (!inited) {
        synchronized(this) { if (!inited) { PDFBoxResourceLoader.init(context.applicationContext); inited = true } }
      }
      val d = try {
        PDDocument.load(file, MemoryUsageSetting.setupTempFileOnly())
      } catch (e: InvalidPasswordException) {
        throw PdfPasswordException()
      }
      return PdfBoxSource(d)
    }
  }
}
