use axum::{extract::Query, routing::get, Router};
use std::{collections::HashMap, net::SocketAddr};

#[tokio::main]
async fn main() {
    // Building our application with a single Route
    let app = Router::new().route("/", get(handler));

    let port = std::env::var("PORT").unwrap_or("80".to_string());
    let port = port.parse::<u16>().unwrap_or_else(|_| {
        eprintln!("Invalid port number: {}", port);
        std::process::exit(1);
    });
    // Run the server with hyper on http://127.0.0.1:3000
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    eprintln!("Listening on http://{}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn handler(Query(params): Query<HashMap<String, String>>) -> String {
    format!("{:?}", params)
}
