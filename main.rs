use axum::{
    routing::{get},
    Router,
    response::IntoResponse,
};
use cron::Schedule;
use reqwest::Client;
use std::{fs, str::FromStr, time::Duration};
use tokio::task;
// use tokio::time::sleep;

use std::env;
use std::sync::Arc;
use std::collections::HashMap;
use std::sync::Mutex;

use std::thread;
use chrono;
use chrono::Utc;
use futures::future::join_all;
use serde_json::json;

type SharedState = Arc<Mutex<HashMap<String, (bool, String)>>>;

#[tokio::main]
async fn main() {
    // Shared state to store website status
    let state: SharedState = Arc::new(Mutex::new(HashMap::new()));

    // Clone state for use in the cron job
    let state_for_cron = state.clone();

    // Start the cron job
    task::spawn(async move {
        run_cron_job(state_for_cron).await;
    });

    // Build the Axum router
    let app = Router::new()
        .route("/healthcheck", get(move || health_check(state.clone())));

    // Start the server
    axum::Server::bind(&"0.0.0.0:10000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn health_check(state: SharedState) -> impl IntoResponse {
    let state = state.lock().unwrap();

    // Log the length of the state
    println!("Number of websites checked: {}", state.len());

    let statuses: Vec<String> = state.iter()
        .map(|(site, &(status, ref time))| format!("{}: {} at {}", site, if status { "OK" } else { "DOWN" }, time))
        .collect();

    statuses.join("\n")
}

async fn run_cron_job(state: SharedState) {
    // Define the cron schedule
    // let expression = "0/5 * * * * *"; // every Five seconds
    let expression = "12 2/5 * * * *"; // every Five minutes since at HH:02, on second: 12
    // let expression = "12 * * * * *"; // every minute on second: 12
    let schedule = Schedule::from_str(expression).expect("Failed to parse CRON expression");

    let client = Client::new();

    let api_tg_bot = env::var("API_TG_BOT").expect("$API_TG_BOT is not set");
    let chat_id = env::var("CHAT_ID").expect("$CHAT_ID is not set");
    let secret_file = env::var("SECRET_FILE").expect("$SECRET_FILE is not set");

    for datetime in schedule.upcoming(chrono::Utc) {
        let now = chrono::Utc::now();

        // let duration = (datetime - now).to_std().unwrap_or(Duration::from_secs(0));
        // sleep(duration).await;
        let until = datetime - now;
        thread::sleep(until.to_std().unwrap());

        println!("Checking...");

        // Read the website list from a file
        let sites = read_websites_from_file(&secret_file);

        // Check each website
        // for site in sites {
        //     let state = state.clone();
        //     let client = client.clone();
        //     task::spawn(async move {
        //         let status = check_website(&client, &site).await;
        //         let time = Utc::now().to_rfc3339();
        //         let mut state = state.lock().unwrap();
        //         state.insert(site, (status, time));
        //     });
        // }

        // Ensure all the websites are checked
        let futures = sites.into_iter().map(|site| {
            let state = state.clone();
            let client = client.clone();
            async move {
                let status = check_website(&client, &site).await;
                let time = Utc::now().to_rfc3339();
                let mut state = state.lock().unwrap();
                state.insert(site, (status, time));
            }
        });

        join_all(futures).await;

        // Prepare the payload
        let payload = {
            let state = state.lock().unwrap();
            let statuses: Vec<String> = state.iter()
                .enumerate()
                .map(|(i, (_site, &(status, ref time)))| format!("{}: {} at {}", i, if status { "OK" } else { "DOWN" }, time))
                .collect();
            statuses.join("\n")
        };

        // Send the POST request
        let _response = Client::new().post(api_tg_bot.clone())
            .json(&json!({
                "method": "sendMessage",
                "chat_id": chat_id.clone(),
                "text": "healthcheck\n".to_owned() + &payload
            }))
            .send()
            .await;

        // Handle the response if needed
    }
}

fn read_websites_from_file(filename: &str) -> Vec<String> {
    fs::read_to_string(filename)
        .unwrap_or_default()
        .lines()
        .map(|s| s.to_string())
        .collect()
}

async fn check_website(client: &Client, url: &str) -> bool {
    client.get(url)
        .send()
        .await
        .map(|res| res.status().is_success())
        .unwrap_or(false)
}
