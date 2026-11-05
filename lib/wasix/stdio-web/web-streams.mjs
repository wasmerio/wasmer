/**
 * Web Streams on the UI/coordinator event loop, never on a blocking task worker.
 * The bridge sends bounded requests to the independent runtime coordinator:
 *   writeInput(Uint8Array) -> Promise<number>        Sink::write (partial)
 *   closeInput() / abortInput(reason)                Sink::close / abort
 *   readOutput("stdout"|"stderr", max) -> Promise<Uint8Array|null>
 *                                                   Source::read (null = EOF)
 *   cancelOutput(name, reason)                      Source::cancel
 *   resize(WindowSize) / hangup()                    existing Signaller methods
 * Cancel/abort callbacks MUST resolve outstanding read/write requests as well.
 * The bridge owns message routing and copies; it must not run runtime Rust on the UI.
 */
export function createStdio(bridge, chunkBytes = 16 * 1024) {
  if (!Number.isSafeInteger(chunkBytes) || chunkBytes <= 0) {
    throw new RangeError("chunkBytes must be a positive safe integer");
  }
  let aborting;
  let inputFailed = false;
  let inputFailure;
  let inputController;
  function abortInput(reason) {
    if (!inputFailed) { inputFailed = true; inputFailure = reason; }
    if (!inputController.signal.aborted) inputController.error(reason);
    aborting ??= Promise.resolve().then(() => bridge.abortInput(reason));
    // Infrastructure failures may precede attaching a waiter. Keep the original rejection.
    aborting.catch(() => {});
    return aborting;
  }
  const stdin = new WritableStream({
    start(controller) {
      inputController = controller;
      // WritableStream.abort waits for an in-flight write before calling the underlying
      // abort hook. Its signal is immediate, so use it to release a capacity-blocked write.
      controller.signal.addEventListener("abort", () => abortInput(controller.signal.reason));
    },
    async write(bytes, controller) {
      try {
        if (!(bytes instanceof Uint8Array)) throw new TypeError("stdin requires Uint8Array");
        for (let offset = 0; offset < bytes.length;) {
          controller.signal.throwIfAborted();
          if (inputFailed) throw inputFailure;
          const chunk = bytes.subarray(offset, offset + chunkBytes);
          const n = await bridge.writeInput(chunk);
          if (inputFailed) throw inputFailure;
          if (!Number.isInteger(n) || n <= 0 || n > chunk.length) {
            throw new RangeError("invalid stdin partial-write count");
          }
          offset += n;
        }
      } catch (error) {
        abortInput(error);
        throw error;
      }
    },
    async close() {
      try { await bridge.closeInput(); }
      catch (error) { abortInput(error); throw error; }
    },
    abort: abortInput,
  }, new CountQueuingStrategy({ highWaterMark: 1 }));

  function output(name) {
    let controller;
    let ended = false;
    let stopping;
    let resolveDrain, rejectDrain;
    const drained = new Promise((resolve, reject) => { resolveDrain = resolve; rejectDrain = reject; });
    drained.catch(() => {});
    function stop(reason = new DOMException("Output canceled", "AbortError")) {
      if (stopping) return stopping;
      if (ended) return Promise.resolve();
      ended = true;
      rejectDrain(reason);
      controller.error(reason);
      stopping = Promise.resolve().then(() => bridge.cancelOutput(name, reason));
      stopping.catch(() => {});
      return stopping;
    }
    const stream = new ReadableStream({
      start(value) { controller = value; },
      async pull() {
        try {
          const bytes = await bridge.readOutput(name, chunkBytes);
          if (ended) return; // A late reply to a canceled read is never enqueued.
          if (bytes === null) {
            ended = true;
            controller.close();
            resolveDrain();
          } else {
            if (!(bytes instanceof Uint8Array) || bytes.length === 0 || bytes.length > chunkBytes) {
              throw new RangeError("invalid output chunk");
            }
            controller.enqueue(bytes);
          }
        } catch (error) {
          if (!ended) {
            // Error the JS stream and release a writer blocked on the Rust queue.
            await stop(error).catch(() => {});
          }
        }
      },
      cancel: stop,
    }, new CountQueuingStrategy({ highWaterMark: 0 }));
    return { stream, drained, stop };
  }
  const out = output("stdout");
  const err = output("stderr");
  const outputDrained = Promise.all([out.drained, err.drained]).then(() => {});
  outputDrained.catch(() => {});
  return {
    stdin, stdout: out.stream, stderr: err.stream, outputDrained,
    // Infrastructure cancellation bypasses stream locks, including an active reader/writer.
    abort(reason = new DOMException("Process canceled", "AbortError")) {
      return Promise.all([abortInput(reason), out.stop(reason), err.stop(reason)]).then(() => {});
    },
    resize({ rows, cols, xpixel = 0, ypixel = 0 }) {
      const size = { rows, cols, xpixel, ypixel };
      for (const value of Object.values(size)) {
        if (!Number.isInteger(value) || value < 0 || value > 65535) {
          throw new RangeError("winsize fields must be u16");
        }
      }
      // Linux drivers/tty/tty_io.c:tty_do_resize sends SIGWINCH; JS owns no signal policy.
      return bridge.resize(size);
    },
    hangup() {
      // Linux drivers/tty/tty_io.c:do_tty_hangup. Retain output for normal exit drain.
      return Promise.all([Promise.resolve().then(() => bridge.hangup()), abortInput(
        new DOMException("Terminal hung up", "AbortError"),
      )]).then(() => {});
    },
  };
}
