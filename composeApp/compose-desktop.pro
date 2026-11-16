# ProGuard rules for the packaged desktop app (createReleaseDistributable). It only removes unused code:
# no renaming and no optimization passes, so stack traces stay readable and Kotlin keeps working.
# Compose Multiplatform adds its own rules for Compose and Skia.

# kotlinx.serialization: generated serializers are found through their companions.
-keepattributes *Annotation*, InnerClasses, Signature, EnclosingMethod
-keepclassmembers @kotlinx.serialization.Serializable class ** {
    *** Companion;
    kotlinx.serialization.KSerializer serializer(...);
}
-keep,includedescriptorclasses class io.parchment.ai.**$$serializer { *; }
-keepclasseswithmembers class io.parchment.ai.** { kotlinx.serialization.KSerializer serializer(...); }

# ServiceLoader lookups: Ktor's JSON support for ContentNegotiation and the Swing main dispatcher. ProGuard can't see
# classes named only in META-INF/services, so each needs a rule; `verify.py final` fails if one is missing (v2.1.2
# shipped without the JSON one and every request failed). The HTTP engine is named in code (providerHttpEngine), so
# nothing scans for it; its container is kept only so Ktor's services file never lists a missing class.
-keep class * implements io.ktor.client.HttpClientEngineContainer { <init>(); }
-keep class * implements io.ktor.serialization.kotlinx.KotlinxSerializationExtensionProvider { <init>(); }
-keep class kotlinx.coroutines.swing.SwingDispatcherFactory { *; }
-keep class kotlinx.coroutines.internal.MainDispatcherFactory { *; }

# The app's entry point.
-keep class io.parchment.ai.MainKt { public static void main(java.lang.String[]); }

# Optional classes referenced by libraries but never used here.
-dontwarn org.slf4j.**
-dontwarn org.conscrypt.**
-dontwarn org.bouncycastle.**
-dontwarn org.openjsse.**
-dontwarn okhttp3.internal.platform.**
-dontwarn com.oracle.svm.**
-dontwarn org.graalvm.**
-dontwarn okhttp3.internal.graal.**
-dontwarn android.**
-dontwarn kotlinx.coroutines.debug.**
-dontwarn io.ktor.util.debug.**
