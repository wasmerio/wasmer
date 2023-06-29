use std::time::Duration;

use fantoccini::Client;
use wasmer_web_tests::{assert_screenshot, browser_test};

#[macro_rules_attribute::apply(browser_test)]
async fn dummy(client: Client) {
    let url = client.current_url().await.unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_screenshot(&client).await.unwrap();
}
