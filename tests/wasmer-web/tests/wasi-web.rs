use std::time::Duration;

use fantoccini::Client;
use predicates::str;
use wasmer_web_tests::{browser_test, ClientExt};

#[macro_rules_attribute::apply(browser_test)]
async fn bash_is_loaded_and_can_show_its_prompt(client: Client) {
    client.wait_for_xterm(str::contains("bash-5.1#")).await;
}

#[macro_rules_attribute::apply(browser_test)]
async fn run_the_ls_command(client: Client) {
    // Wait for xterm to be ready
    client.wait_for_xterm(str::contains("bash-5.1#")).await;

    let stdin = client
        .find(fantoccini::Locator::Css("textarea.xterm-helper-textarea"))
        .await
        .unwrap();
    stdin.send_keys("ls\n").await.unwrap();

    tokio::time::sleep(Duration::from_secs(2)).await;

    client
        .wait_for_xterm(str::contains("bash-5.1# ls\nbin\ndev\netc\ntmp\nusrx"))
        .await;
}
