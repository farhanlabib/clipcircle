plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// The app takes its version from the Rust workspace, so one release tag
// versions every platform.
val appVersion = Regex("""(?m)^version = "([^"]+)"""")
    .find(rootDir.resolve("../Cargo.toml").readText())!!
    .groupValues[1]
val (major, minor, patch) = appVersion.substringBefore('-').split('.').map { it.toInt() }

// Release builds are signed with the key named in the environment (the
// release workflow sets it from repository secrets). Without one they use the
// debug key, which is fine for trying a build but can't update an installed
// release.
val releaseKeystore: String? = System.getenv("ANDROID_KEYSTORE")

android {
    namespace = "dev.farhanlabib.clipcircle"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.farhanlabib.clipcircle"
        minSdk = 29
        targetSdk = 35
        versionCode = major * 10000 + minor * 100 + patch
        versionName = appVersion
    }

    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName(if (releaseKeystore != null) "release" else "debug")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    // UniFFI's Kotlin bindings call the Rust library through JNA.
    implementation("net.java.dev.jna:jna:5.15.0@aar")
}
