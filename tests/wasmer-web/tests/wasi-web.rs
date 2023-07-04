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
    // Wait for xterm to be ready
    client.wait_for_xterm(str::contains(PROMPT)).await;

    let output = client.execute_command("ls", PROMPT).await;

    assert_eq!(output, "bin\ndev\netc\ntmp\nusr");
}

#[macro_rules_attribute::apply(browser_test)]
async fn pipe_between_commands(client: Client) {
    // Wait for xterm to be ready
    client.wait_for_xterm(str::contains(PROMPT)).await;

    let output = client.execute_command("ls | wc", PROMPT).await;

    assert_eq!(output, "      5       5      20");
}
