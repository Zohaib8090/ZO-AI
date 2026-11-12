import java.util.Properties

plugins {
  alias(libs.plugins.android.application)
  alias(libs.plugins.kotlin.compose)
  alias(libs.plugins.kotlin.serialization)
  alias(libs.plugins.roborazzi)
}

// google-services.json is NOT in the repository (it is the owner's Firebase file for package org.zohara.zoai). The plugin is applied only
// when the file exists, so the app builds and runs without it (sign-in then shows "Sign-in is not configured in this build").
val hasGoogleServices = file("google-services.json").exists()
if (hasGoogleServices) {
  apply(plugin = "com.google.gms.google-services")
}

android {
  namespace = "org.zohara.zoai"
  compileSdk { version = release(36) { minorApiLevel = 1 } }

  defaultConfig {
    applicationId = "org.zohara.zoai"
    minSdk = 26
    targetSdk = 36
    versionCode = (System.getenv("ZO_VERSION_CODE") ?: "1").toInt()
    versionName = "0.1.${System.getenv("ZO_VERSION_CODE") ?: "1"}"
    buildConfigField("String", "DEFAULT_SERVER_URL", "\"https://zo-ai-server.onrender.com\"")
    buildConfigField("boolean", "FIREBASE_CONFIGURED", hasGoogleServices.toString())
    // the Web client id of the Firebase project (Google sign-in): the owner puts it in local.properties as ZO_WEB_CLIENT_ID=... or the
    // environment; empty means Google sign-in says it is not set up.
    val localProps = Properties().apply {
      val f = rootProject.file("local.properties")
      if (f.exists()) f.inputStream().use { load(it) }
    }
    val webClient = (System.getenv("ZO_WEB_CLIENT_ID") ?: localProps.getProperty("ZO_WEB_CLIENT_ID") ?: "")
    buildConfigField("String", "GOOGLE_WEB_CLIENT_ID", "\"$webClient\"")
    testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
  }

  buildTypes {
    release {
      isMinifyEnabled = false
      proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
    }
    debug {
      applicationIdSuffix = ""
    }
  }
  compileOptions {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
  }
  buildFeatures {
    compose = true
    buildConfig = true
  }
  testOptions {
    unitTests {
      isIncludeAndroidResources = true
      isReturnDefaultValues = true
    }
  }
  lint {
    abortOnError = false
    checkReleaseBuilds = false
  }
  packaging { resources.excludes += setOf("/META-INF/{AL2.0,LGPL2.1}", "/META-INF/INDEX.LIST", "/META-INF/io.netty.versions.properties") }
}

dependencies {
  implementation(platform(libs.androidx.compose.bom))
  implementation(libs.androidx.activity.compose)
  implementation(libs.androidx.core.ktx)
  implementation(libs.androidx.compose.ui)
  implementation(libs.androidx.compose.ui.graphics)
  implementation(libs.androidx.compose.ui.text)
  implementation(libs.androidx.compose.ui.tooling.preview)
  implementation(libs.androidx.compose.foundation)
  implementation(libs.androidx.compose.material3)
  implementation(libs.androidx.lifecycle.runtime.compose)
  implementation(libs.androidx.lifecycle.runtime.ktx)
  implementation(libs.androidx.lifecycle.viewmodel.compose)
  implementation(libs.androidx.navigation.compose)
  implementation(libs.androidx.datastore.preferences)
  implementation(libs.androidx.browser)
  implementation(libs.kotlinx.coroutines.android)
  implementation(libs.kotlinx.coroutines.core)
  implementation(libs.kotlinx.coroutines.play.services)
  implementation(libs.kotlinx.serialization.json)
  implementation(libs.okhttp)
  implementation(platform(libs.firebase.bom))
  implementation(libs.firebase.auth)
  implementation(libs.androidx.credentials)
  implementation(libs.androidx.credentials.play.services)
  implementation(libs.googleid)

  testImplementation(libs.junit)
  testImplementation(libs.kotlinx.coroutines.test)
  testImplementation(libs.okhttp.mockwebserver)
  testImplementation(libs.robolectric)
  testImplementation(libs.androidx.test.core)
  testImplementation(libs.androidx.test.junit)
  testImplementation(platform(libs.androidx.compose.bom))
  testImplementation(libs.androidx.compose.ui.test.junit4)
  testImplementation(libs.roborazzi)
  testImplementation(libs.roborazzi.compose)
  testImplementation(libs.roborazzi.junit.rule)
  debugImplementation(libs.androidx.compose.ui.test.manifest)
  debugImplementation(libs.androidx.compose.ui.tooling)
}
