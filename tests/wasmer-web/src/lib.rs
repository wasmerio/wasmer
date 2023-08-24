use std::{
    io::ErrorKind,
    net::SocketAddr,
    panic::Location,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Error};
use fantoccini::Client;
use futures::{channel::oneshot::Sender, Future};
use predicates::Predicate;
use tempfile::{NamedTempFile, TempDir};
use tokio::{
    net::TcpStream,
    process::{Child, Command},
};

pub const WEBPACK_DEV_SERVER_URL: &str = "http://localhost:9000/";
const RECORDING_INTERVAL: Duration = Duration::from_millis(250);
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// Define a browser test.
///
/// ```rust
/// # use wasmer_web_tests::browser_test;
/// #[macro_rules_attribute::apply(browser_test)]
/// async fn it_works(client: Client) {
///     let url = client.current_url().await.unwrap();
///     assert_eq!(url.as_str(), "http://localhost:9000/");
/// }
/// ```
#[macro_export]
macro_rules! browser_test {
    ($(#[$meta:meta])* async fn $name:ident($client_var:ident : $client_ty:ty) $body:block) => {
        #[test]
        $( #[$meta] )*
        fn $name() {
            $crate::run_browser_test(|$client_var: $client_ty| async move { $body });
        }
    };
}

/// The real meat & potatoes of our test suite.
///
/// This spins up a Tokio runtime, starts a headless Chrome browser, runs the
/// provided code (presumably a test), then tears everything down.
///
/// To help debug failing tests, the [`Session`] will also record screenshots to
/// disk so users can see what the browser saw. The path to this folder will be
/// printed after the test fails.
#[doc(hidden)]
pub fn run_browser_test<F, Fut>(thunk: F)
where
    F: FnOnce(Client) -> Fut,
    Fut: std::future::Future,
{
    let runtime = tokio::runtime::Runtime::new().unwrap();

    runtime.block_on(async {
        let session = Session::begin().await.unwrap();
        let client = session.client.clone();

        client.goto(WEBPACK_DEV_SERVER_URL).await.unwrap();
        client
            .wait()
            .for_url(WEBPACK_DEV_SERVER_URL.parse().unwrap())
            .await
            .unwrap();

        let result =
            futures::future::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(thunk(client)))
                .await;
        session.shutdown(result.is_err()).await.unwrap();

        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    });
}

/// All state associated with a browser test.
#[derive(Debug)]
struct Session {
    /// The chromedriver process.
    driver: Child,
    /// A fantoccini client.
    client: Client,
    /// The file all chromedriver logs are written to.
    logs: NamedTempFile,
    /// The directory screenshots are saved to for debugging.
    recording_dir: TempDir,
    /// Tell the background job to stop automatically recording debug
    /// screenshots.
    stop_recording: Sender<()>,
}

impl Session {
    async fn begin() -> Result<Self, Error> {
        let logs = NamedTempFile::new().context("Unable to create a temporary file")?;
        let addr = random_port_number().await?;

        let mut driver = Command::new("chromedriver")
            .arg(format!("--port={}", addr.port()))
            .stderr(logs.as_file().try_clone()?)
            .stdout(logs.as_file().try_clone()?)
            .spawn()
            .context("Unable to start `chromedriver`. Is it installed?")?;

        // Wait until chromedriver is ready
        let timeout = tokio::time::sleep(Duration::from_millis(2000));
        tokio::select! {
            result = driver.wait() => {
                let exit_code = result?;
                let (_, path) = logs.keep()?;
                anyhow::bail!(
                    "The chromedriver exited prematurely with exit code {exit_code}. Check {} for more details.",
                    path.display(),
                );
            }
            _ = wait_until_started(addr) => {
                // Chromedriver is ready for us now.
            }
            _ = timeout => {
                anyhow::bail!("Timeout waiting for chromedriver to start");
            }
        }

        let connect_addr = format!("http://{addr}/");

        let client = fantoccini::ClientBuilder::native()
            .capabilities(capabilities())
            .connect(&connect_addr)
            .await?;

        let (recording_dir, stop_recording) = record_browser(client.clone())?;

        Ok(Session {
            driver,
            client,
            logs,
            recording_dir,
            stop_recording,
        })
    }

    async fn shutdown(mut self, failed: bool) -> Result<(), Error> {
        let _ = self.stop_recording.send(());

        self.client.close_window().await?;
        self.client.close().await?;

        match tokio::time::timeout(Duration::from_millis(500), self.driver.wait()).await {
            Ok(Ok(exit_status)) if exit_status.success() => {
                // exited cleanly
            }
            Ok(Ok(exit_status)) => {
                eprintln!("Chromedriver exited unsuccessfully. Exit code: {exit_status}");
            }
            Ok(Err(e)) => {
                return Err(e.into());
            }
            Err(_) => {
                // We gave it a chance to exit gracefully, but it didn't. Let's
                // pull the plug.
                eprintln!("Forcefully killing the chrome driver");
                self.driver.kill().await?;
            }
        }

        let logs = std::fs::read(self.logs.path())?;
        let logs = String::from_utf8_lossy(&logs);
        eprintln!("==== Chromedriver Logs ====");
        eprintln!("{logs}");
        eprintln!("==== End Chromedriver Logs ====");

        if failed {
            let recording_dir = self.recording_dir.into_path();
            eprintln!("Session recordings: {}", recording_dir.display());
        }

        Ok(())
    }
}

fn record_browser(client: Client) -> Result<(TempDir, Sender<()>), Error> {
    let recording_dir = TempDir::new()?;
    let (stop_recording, tx) = futures::channel::oneshot::channel::<()>();

    let recording_dir_path = recording_dir.path().to_path_buf();
    tokio::spawn(async move {
        let screenshots = async {
            let mut timer = tokio::time::interval(RECORDING_INTERVAL);
            let started = Instant::now();

            loop {
                let tick = timer.tick().await;
                let bytes = client.screenshot().await?;
                let run_time = tick.duration_since(started.into());
                let filename = format!("{}.{}.png", run_time.as_secs(), run_time.subsec_millis());
                tokio::fs::write(recording_dir_path.join(filename), bytes).await?;
            }
        };

        tokio::select! {
            _ = tx => {
                // Return. Stopping the recording.
            },
            result = screenshots => {
                let result: Result<(), Error> = result;
                if let Err(e) = result {
                    eprintln!(
                        "An error occurred while capturing screenshots to \"{}\": {e:?}",
                        recording_dir_path.display(),
                    );
                }
            },
        }
    });

    Ok((recording_dir, stop_recording))
}

async fn random_port_number() -> Result<SocketAddr, Error> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let local_addr = listener.local_addr()?;
    drop(listener);
    Ok(local_addr)
}

async fn wait_until_started(target: SocketAddr) -> Result<(), Error> {
    loop {
        match TcpStream::connect(target).await {
            Ok(_) => return Ok(()),
            Err(e) if matches!(e.kind(), ErrorKind::ConnectionRefused) => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }
}

fn capabilities() -> serde_json::Map<String, serde_json::Value> {
    let caps = serde_json::json!({
        "browserName":"chrome",
        "goog:chromeOptions": { "args": ["--headless=new"] },
    });

    match caps {
        serde_json::Value::Object(caps) => caps,
        _ => unreachable!(),
    }
}

#[track_caller]
pub fn assert_screenshot(client: &Client) -> impl Future<Output = Result<(), Error>> + '_ {
    let caller = Location::caller();

    async move {
        let _screenshot = client
            .screenshot()
            .await
            .context("Unable to capture the screenshot")?;

        let caller_file = Path::new(caller.file())
            .canonicalize()
            .with_context(|| format!("Unable to canonicalize \"{}\"", caller.file()))?;
        let parent = caller_file
            .parent()
            .context("Unable to determine the file's folder")?;

        let _snapshot_dir = parent.join("snapshots");

        todo!("Diff the screenshot, insta-style");
    }
}

/// Extra methods added to [`Client`] for use with browser tests.
#[async_trait::async_trait]
pub trait ClientExt {
    /// Read the contents of the `xterm.js` terminal.
    async fn read_terminal(&self) -> Result<String, Error>;

    /// Wait until the contents of the terminal satisfies a particular
    /// [`Predicate`].
    async fn wait_for_xterm(&self, predicate: impl Predicate<str> + Send) -> String;

    /// Wait until the contents of the terminal satisfies a particular
    /// [`Predicate`].
    async fn wait_for_xterm_with_timeout(
        &self,
        timeout: Duration,
        predicate: impl Predicate<str> + Send,
    ) -> String;

    #[track_caller]
    async fn assert_screenshot(&self) -> Result<(), Error>;

    /// Run a command and wait for it to finish (i.e. the next prompt is shown).
    ///
    /// ## Caveats
    ///
    /// Everything is built on top of [`ClientExt::read_terminal()`] and string
    /// manipulation, so a lot of caveats apply.
    ///
    /// - Make sure your comand doesn't generate so much output that the
    ///   original prompt disappears off the screen
    async fn execute_command(&self, cmd: &str, prompt: &str) -> String;

    /// Run a command and wait for it to finish.
    ///
    /// See [`ClientExt::execute_command()`] for more.
    async fn execute_command_with_timeout(
        &self,
        cmd: &str,
        prompt: &str,
        timeout: Duration,
    ) -> String;
}

#[async_trait::async_trait]
impl ClientExt for Client {
    async fn read_terminal(&self) -> Result<String, Error> {
        let js = r#"
            const xterm = window.xterm;

            if (!xterm) {
                return "";
            }

            xterm.selectAll();
            const selection = xterm.getSelection();
            xterm.clearSelection();
            return selection;
        "#;
        match self.execute(js, Vec::new()).await? {
            serde_json::Value::String(mut s) => {
                // the terminal adds a bunch of newlines to the end. Let's get
                // rid of them so the user doesn't see output scroll off the
                // screen when printing the terminal output.
                let len = s.trim_end().len();
                s.truncate(len);
                Ok(s)
            }
            other => {
                anyhow::bail!("Unable to deserialize {other:?} as a string")
            }
        }
    }

    async fn wait_for_xterm(&self, predicate: impl Predicate<str> + Send) -> String {
        self.wait_for_xterm_with_timeout(DEFAULT_TIMEOUT, predicate)
            .await
    }

    async fn wait_for_xterm_with_timeout(
        &self,
        timeout: Duration,
        predicate: impl Predicate<str> + Send,
    ) -> String {
        let cutoff = Instant::now() + timeout;
        let mut previous_output = String::new();

        loop {
            tokio::select! {
                result = self.read_terminal() => {
                    match result {
                        Ok(contents) if predicate.eval(&contents) => {
                            return contents;
                        }
                        Ok(contents) => {
                            previous_output = contents;
                         }
                        Err(e) => {
                            panic!("{e:?}");
                        }
                    }
                }
                _ = tokio::time::sleep_until(cutoff.into()) => {
                    eprintln!("=== Terminal Contents ===");
                    eprintln!("{previous_output}");
                    eprintln!("=== End Terminal Contents ===");

                    panic!("Timed out");
                }
            }
        }
    }

    #[track_caller]
    async fn assert_screenshot(&self) -> Result<(), Error> {
        assert_screenshot(self).await
    }

    async fn execute_command(&self, cmd: &str, prompt: &str) -> String {
        self.execute_command_with_timeout(cmd, prompt, DEFAULT_TIMEOUT)
            .await
    }

    async fn execute_command_with_timeout(
        &self,
        cmd: &str,
        prompt: &str,
        timeout: Duration,
    ) -> String {
        let previous_output = self
            .read_terminal()
            .await
            .expect("Unable to read the terminal");

        let stdin = self
            .find(fantoccini::Locator::Css("textarea.xterm-helper-textarea"))
            .await
            .expect("Unable to find the xterm textarea");

        stdin
            .send_keys(cmd)
            .await
            .expect("Unable to send the command");
        stdin
            .send_keys("\n")
            .await
            .expect("Unable to send a newline");

        let terminal_contents = self
            .wait_for_xterm_with_timeout(
                timeout,
                predicates::function::function(|s: &str| {
                    // First, trim away anything before/including the previous output
                    let Some((_, new_content)) = s.split_once(&previous_output) else {
                        return false;
                    };

                    // Now, we want to get the content after the command
                    let Some((_before, after)) = new_content.split_once(cmd) else {
                        return false;
                    };

                    after.trim_start_matches('\n').contains(prompt)
                }),
            )
            .await;

        extract_command_output(&terminal_contents, prompt).to_string()
    }
}

fn extract_command_output<'a>(terminal: &'a str, prompt: &str) -> &'a str {
    // Note: we assume the terminal output looks like this:
    //   prompt $command
    //   lines of output
    //   lines of output
    //   lines of output
    //   lines of output
    //   prompt
    //   optional trailing stuff
    //
    // We want those "lines of output"
    let chunks: Vec<_> = terminal.split(prompt).collect();
    match chunks.as_slice() {
        [.., output, _optional_trailing_stuff] => match output.find('\n') {
            // Get rid of the leading prompt and command
            Some(first_newline) => &output[first_newline + 1..],
            _ => output,
        },
        _ => panic!("Terminal output wasn't in the expected format"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_output() {
        let src = r#"
██╗    ██╗ █████╗ ███████╗███╗   ███╗███████╗██████╗    ███████╗██╗  ██╗
██║    ██║██╔══██╗██╔════╝████╗ ████║██╔════╝██╔══██╗   ██╔════╝██║  ██║
██║ █╗ ██║███████║███████╗██╔████╔██║█████╗  ██████╔╝   ███████╗███████║
██║███╗██║██╔══██║╚════██║██║╚██╔╝██║██╔══╝  ██╔══██╗   ╚════██║██╔══██║
╚███╔███╔╝██║  ██║███████║██║ ╚═╝ ██║███████╗██║  ██║██╗███████║██║  ██║
    ╚══╝╚══╝ ╚═╝  ╚═╝╚══════╝╚═╝     ╚═╝╚══════╝╚═╝  ╚═╝╚═╝╚══════╝╚═╝  ╚═╝
    QUICK START:                         MORE INFO:
• Wasmer commands:  wasmer            • Usage Information: help
• Core utils:       coreutils         • About Wasmer: about wasmer
• Pipe: echo blah | cat
wcbash-5.1# wc


wc: 'standard input': Interrupted system call (os error 27)
        1       1       4
bash-5.1# ls | wc
        5       5      20
bash-5.1#
       "#;

        let output = extract_command_output(src, "bash-5.1#");

        assert_eq!(output, "        5       5      20\n");
    }
}
