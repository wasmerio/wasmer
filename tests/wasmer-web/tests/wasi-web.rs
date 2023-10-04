use std::time::Duration;

use fantoccini::Client;
use predicates::str;
use wasmer_web_tests::{browser_test, ClientExt};

const PROMPT: &str = "bash-5.1#";

#[macro_rules_attribute::apply(browser_test)]
async fn the_welcome_message_is_shown(client: Client) {
    let top_line_of_wasmer_banner =
        "██╗    ██╗ █████╗ ███████╗███╗   ███╗███████╗██████╗    ███████╗██╗  ██╗";

    client
        .wait_for_xterm(str::contains(top_line_of_wasmer_banner))
        .await;
}

#[macro_rules_attribute::apply(browser_test)]
async fn bash_is_loaded_and_can_show_its_prompt(client: Client) {
    client.wait_for_xterm(str::contains(PROMPT)).await;
}

#[macro_rules_attribute::apply(browser_test)]
async fn run_the_ls_command(client: Client) {
    client.wait_for_xterm(str::contains(PROMPT)).await;

    let output = client.execute_command("ls", PROMPT).await;

    assert_eq!(output, "bin\ndev\netc\ntmp\nusr\n");
}

#[macro_rules_attribute::apply(browser_test)]
async fn pipe_between_commands(client: Client) {
    client.wait_for_xterm(str::contains(PROMPT)).await;

    let output = client.execute_command("ls | wc", PROMPT).await;

    assert_eq!(output, "      5       5      20\n");
}

#[macro_rules_attribute::apply(browser_test)]
async fn run_a_webc_package_that_involves_the_filesystem(client: Client) {
    // Wait for xterm to be ready
    client.wait_for_xterm(str::contains(PROMPT)).await;

    let output = client
        .execute_command_with_timeout(
            "wasmer python/python@0.1.0 -c 'import sys; print(sys.version_info)'",
            PROMPT,
            Duration::from_secs(30),
        )
        .await;

    assert_eq!(
        output,
        "sys.version_info(major=3, minor=6, micro=7, releaselevel='final', serial=0)\n",
    );
}

#[ignore] // FIXME: This test is flaky on CI - @Michael-F-Bryan
#[macro_rules_attribute::apply(browser_test)]
async fn pure_webc_package(client: Client) {
    client.wait_for_xterm(str::contains(PROMPT)).await;

    let output = client
        .execute_command_with_timeout(
            "wasmer run wasmer/hello --version",
            PROMPT,
            Duration::from_secs(30),
        )
        .await;
    tokio::time::sleep(Duration::from_secs(1)).await;

    assert_eq!(output.trim(), "static-web-server 2.14.2");
}
