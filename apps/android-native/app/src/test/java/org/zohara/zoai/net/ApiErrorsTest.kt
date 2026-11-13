package org.zohara.zoai.core.net

import java.net.SocketTimeoutException
import java.net.UnknownHostException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ApiErrorsTest {
  @Test fun unauthorized() {
    val e = ApiErrors.fromHttp(401, """{"error":"x"}""", null)
    assertEquals(ErrorKind.Unauthorized, e.kind)
    assertEquals(ApiErrors.SIGNED_OUT, e.message)
  }

  @Test fun termsRequiredCarriesVersion() {
    val e = ApiErrors.fromHttp(403, """{"error":"Accept first","terms_required":true,"version":"2026-11-01"}""", null)
    assertEquals(ErrorKind.TermsRequired, e.kind)
    assertEquals("2026-11-01", e.termsVersion)
  }

  @Test fun otherForbiddenIsBlocked() {
    assertEquals(ErrorKind.Blocked, ApiErrors.fromHttp(403, """{"error":"Blocked by admin"}""", null).kind)
    assertEquals("Blocked by admin", ApiErrors.fromHttp(403, """{"error":"Blocked by admin"}""", null).message)
  }

  @Test fun rateLimitUsesHeaderAndPlural() {
    val e = ApiErrors.fromHttp(429, "{}", "7")
    assertEquals(ErrorKind.RateLimited, e.kind)
    assertEquals(7, e.retryAfterSec)
    assertEquals("You are going a little fast. Please wait 7 seconds and try again.", e.message)
    assertEquals("You are going a little fast. Please wait 1 second and try again.", ApiErrors.fromHttp(429, "{}", "1").message)
    assertEquals("You are going a little fast. Please wait a moment and try again.", ApiErrors.fromHttp(429, "{}", null).message)
  }

  @Test fun statusMapping() {
    assertEquals(ErrorKind.Deleted, ApiErrors.fromHttp(410, "{}", null).kind)
    assertEquals(ErrorKind.TooLarge, ApiErrors.fromHttp(413, "", null).kind)
    assertEquals(ErrorKind.Busy, ApiErrors.fromHttp(503, "", null).kind)
    assertEquals(ErrorKind.NotFound, ApiErrors.fromHttp(404, "not json", null).kind)
    assertEquals(ErrorKind.Rejected, ApiErrors.fromHttp(400, """{"error":"bad thing"}""", null).kind)
    assertEquals(ErrorKind.Server, ApiErrors.fromHttp(500, "<html>", null).kind)
  }

  @Test fun opFailures() {
    assertEquals(ErrorKind.UnknownOp, ApiErrors.fromOpFailure("unknown request: model_test").kind)
    assertEquals(ApiErrors.OLD_SERVER, ApiErrors.fromOpFailure("unknown request: model_test").message)
    assertEquals("nope", ApiErrors.fromOpFailure("nope").message)
    assertEquals(ApiErrors.GENERIC, ApiErrors.fromOpFailure("").message)
  }

  @Test fun networkExceptions() {
    assertEquals(ErrorKind.Offline, ApiErrors.fromThrowable(UnknownHostException("x")).kind)
    assertEquals(ErrorKind.Timeout, ApiErrors.fromThrowable(SocketTimeoutException("timeout")).kind)
    assertEquals(ErrorKind.Server, ApiErrors.fromThrowable(IllegalStateException("odd")).kind)
    assertNull(ApiErrors.fromHttp(401, "", null).retryAfterSec)
  }
}
