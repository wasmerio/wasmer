FROM ubuntu:19.10

RUN dpkg --add-architecture i386 && \
    apt-get update && \
    apt-get install -y --no-install-recommends \
  file \
  curl \
  ca-certificates \
  python \
  unzip \
  expect \
  openjdk-8-jre \
  libstdc++6:i386 \
  libpulse0 \
  gcc \
  libc6-dev \
  make \
  cmake # cmake is necessary to build wabt

WORKDIR /android/
COPY android* /android/

ENV ANDROID_ARCH=aarch64
ENV PATH=$PATH:/android/sdk/tools:/android/sdk/platform-tools

RUN sh /android/android-install-ndk.sh $ANDROID_ARCH
RUN sh /android/android-install-sdk.sh $ANDROID_ARCH
ENV ANDROID_NDK_HOME=/android/ndk

RUN mv /root/.android /tmp
RUN chmod 777 -R /tmp/.android
RUN chmod 755 /android/sdk/tools/* /android/sdk/emulator/qemu/linux-x86_64/*

ENV PATH=$PATH:/rust/bin:$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin \
    CARGO_TARGET_AARCH64_LINUX_ANDROID_AR=aarch64-linux-android-ar \
    CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=aarch64-linux-android28-clang++ \
    CC_aarch64_linux_android=aarch64-linux-android28-clang \
    CXX_aarch64_linux_android=aarch64-linux-android28-clang++ \
    CARGO_TARGET_AARCH64_LINUX_ANDROID_RUNNER=/tmp/runtest \
    HOME=/tmp

ADD runtest-android.rs /tmp/runtest.rs

ENTRYPOINT [ \
  "bash", \
  "-c", \
  # set SHELL so android can detect a 64bits system, see
  # http://stackoverflow.com/a/41789144
  "SHELL=/bin/dash /android/sdk/emulator/emulator @aarch64 -no-window & \
   rustc /tmp/runtest.rs -o /tmp/runtest && \
   exec \"$@\"", \
  "--" \
]
