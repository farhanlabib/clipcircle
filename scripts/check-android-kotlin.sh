#!/usr/bin/env bash
# Type-checks the Android app's Kotlin code without the Android SDK, for when
# CI is off. It compiles against Robolectric's copy of the Android 15
# framework and the UniFFI bindings for the current Rust code. It catches
# Kotlin errors, not resource, manifest or packaging problems.
#
# Needs a JDK (17+), curl and cargo. Downloads about 250 MB of jars from Maven
# Central once, into $CACHE (default ~/.cache/clipcircle-ktc).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
CACHE=${CACHE:-$HOME/.cache/clipcircle-ktc}
TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
MAVEN=https://repo1.maven.org/maven2
KOTLIN=2.0.21 # keep in step with android/build.gradle.kts
ANDROID_ALL=15-robolectric-12650502

JARS=(
  "org/jetbrains/kotlin/kotlin-compiler-embeddable/$KOTLIN/kotlin-compiler-embeddable-$KOTLIN.jar"
  "org/jetbrains/kotlin/kotlin-stdlib/$KOTLIN/kotlin-stdlib-$KOTLIN.jar"
  "org/jetbrains/kotlin/kotlin-script-runtime/$KOTLIN/kotlin-script-runtime-$KOTLIN.jar"
  "org/jetbrains/kotlin/kotlin-daemon-embeddable/$KOTLIN/kotlin-daemon-embeddable-$KOTLIN.jar"
  "org/jetbrains/kotlin/kotlin-reflect/1.6.10/kotlin-reflect-1.6.10.jar"
  "org/jetbrains/intellij/deps/trove4j/1.0.20200330/trove4j-1.0.20200330.jar"
  "org/jetbrains/kotlinx/kotlinx-coroutines-core-jvm/1.6.4/kotlinx-coroutines-core-jvm-1.6.4.jar"
  "org/jetbrains/annotations/13.0/annotations-13.0.jar"
  "net/java/dev/jna/jna/5.15.0/jna-5.15.0.jar"
  "org/robolectric/android-all/$ANDROID_ALL/android-all-$ANDROID_ALL.jar"
)

mkdir -p "$CACHE/jars"
compiler_cp=""
for path in "${JARS[@]}"; do
  jar="$CACHE/jars/$(basename "$path")"
  if [ ! -s "$jar" ]; then
    echo "downloading $(basename "$path")"
    curl -sfL --retry 5 --retry-all-errors -o "$jar.part" "$MAVEN/$path"
    mv "$jar.part" "$jar"
  fi
  case "$jar" in
    *android-all* | *jna-*) ;;
    *) compiler_cp="$compiler_cp:$jar" ;;
  esac
done

echo "generating Kotlin bindings"
cd "$ROOT"
cargo build -q -p clip-ffi --release
case "$(uname)" in
  Darwin) lib=libclip_ffi.dylib ;;
  *) lib=libclip_ffi.so ;;
esac
rm -rf "$CACHE/gen" "$CACHE/out"
cargo run -q -p clip-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library "$TARGET/release/$lib" --language kotlin --no-format --out-dir "$CACHE/gen"

# A stand-in for the R class the Android build would generate.
res=android/app/src/main/res
{
  echo "package dev.farhanlabib.clipcircle"
  echo "object R {"
  echo "  object string {"
  grep -o '<string name="[a-z_]*"' "$res/values/strings.xml" | sed 's/.*name="\(.*\)"/    const val \1 = 0/'
  echo "  }"
  echo "  object drawable {"
  for f in "$res"/drawable/*; do echo "    const val $(basename "${f%.*}") = 0"; done
  echo "  }"
  echo "}"
} >"$CACHE/gen/R.kt"

echo "type-checking"
jars="$CACHE/jars"
# The framework jar lacks the SDK's platform-type annotations, so its
# nullability markers are ignored.
output=$(java -cp "${compiler_cp#:}" org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
  -no-stdlib -no-reflect -jvm-target 17 \
  -Xnullability-annotations=@android.annotation:ignore \
  -classpath "$jars/android-all-$ANDROID_ALL.jar:$jars/jna-5.15.0.jar:$jars/kotlin-stdlib-$KOTLIN.jar" \
  -d "$CACHE/out" \
  $(find android/app/src/main/java -name '*.kt') "$CACHE"/gen/uniffi/clip_ffi/*.kt "$CACHE/gen/R.kt" 2>&1 || true)
if grep -qE "error:|exception:" <<<"$output"; then
  grep -E -A3 "error:|exception:" <<<"$output"
  exit 1
fi
echo "Kotlin sources type-check"
