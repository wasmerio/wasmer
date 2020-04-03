FROM ubuntu:19.04

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
  ca-certificates \
  curl \
  gcc \
  g++ \
  libc-dev \
  python \
  unzip \
  make \
  cmake # cmake is necessary to build wabt

WORKDIR /android/
ENV ANDROID_ARCH=x86_64
COPY android-install-ndk.sh /android/
RUN sh /android/android-install-ndk.sh $ANDROID_ARCH
ENV ANDROID_NDK_HOME=/android/ndk/

# We do not run x86_64-linux-android tests on an android emulator.
# See ci/android-sysimage.sh for informations about how tests are run.
COPY android-sysimage.sh /android/
RUN bash /android/android-sysimage.sh x86_64 x86_64-24_r07.zip

ENV PATH=$PATH:/rust/bin:$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin \
    CARGO_TARGET_X86_64_LINUX_ANDROID_AR=x86_64-linux-android-ar \
    CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER=x86_64-linux-android28-clang++ \
    CC_x86_64_linux_android=x86_64-linux-android28-clang \
    CXX_x86_64_linux_android=x86_64-linux-android28-clang++ \
    LD_LIBRARY_PATH=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/x86_64-linux-android/ \
    HOME=/tmp
