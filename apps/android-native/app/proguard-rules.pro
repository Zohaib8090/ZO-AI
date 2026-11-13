# R8 rules for the release build (minify + resource shrinking).

# kotlinx.serialization: keep the generated serializers and the companion lookups
-keepattributes *Annotation*, InnerClasses, Signature, EnclosingMethod
-dontnote kotlinx.serialization.**
-keepclassmembers class kotlinx.serialization.json.** { *** Companion; }
-keepclasseswithmembers class kotlinx.serialization.json.** { kotlinx.serialization.KSerializer serializer(...); }
-keep,includedescriptorclasses class org.zohara.agent.**$$serializer { *; }
-keepclassmembers class org.zohara.agent.** { *** Companion; }
-keepclasseswithmembers class org.zohara.agent.** { kotlinx.serialization.KSerializer serializer(...); }

# Firebase (Auth, Messaging) and Google Play services ship their own consumer rules; keep the messaging service and receiver by name
-keep class org.zohara.agent.media.notify.ZoMessagingService { *; }
-keep class org.zohara.agent.media.notify.ApprovalReceiver { *; }
-keep class com.google.firebase.** { *; }
-dontwarn com.google.firebase.**
-keep class androidx.credentials.** { *; }
-keep class com.google.android.libraries.identity.googleid.** { *; }

# OkHttp / Okio
-dontwarn okhttp3.internal.platform.**
-dontwarn org.conscrypt.**
-dontwarn org.bouncycastle.**
-dontwarn org.openjsse.**

# PdfBox-Android (reads PDF text) uses reflection and optional codecs
-keep class com.tom_roush.pdfbox.** { *; }
-dontwarn com.tom_roush.pdfbox.**
-dontwarn com.gemalto.jp2.**
-dontwarn javax.xml.stream.**
-dontwarn org.apache.**
