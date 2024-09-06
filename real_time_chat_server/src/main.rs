use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use warp::http::Uri;
use warp::Filter;

// Our global state
type Users = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<warp::ws::Message>>>>;

#[tokio::main]
async fn main() {
    // Create our global state
    let users = Users::default();
    // Create a channel for broadcasting messages to all connected clients
    let (tx, _rx) = broadcast::channel(100);

    // Define our WebSocket route
    let chat = warp::path("chat")
        .and(warp::ws())
        .and(warp::any().map(move || users.clone()))
        .and(warp::any().map(move || tx.clone()))
        .map(|ws: warp::ws::Ws, users, tx| {
            ws.on_upgrade(move |socket| user_connected(socket, users, tx))
        });

    // Serve static files
    let files = warp::path("static")
        .and(warp::fs::dir("static"))
        .with(warp::log::custom(|info| {
            println!("Serving static file: {:?}", info.path());
        }));

    // Serve index.html at the root
    let index = warp::path::end()
        .and(warp::fs::file("static/index.html"))
        .with(warp::log::custom(|info| {
            println!("Serving root: {:?}", info.path());
        }));

    // Redirect "/" to "/static/index.html"
    let redirect =
        warp::path::end().map(|| warp::redirect::temporary(Uri::from_static("/static/index.html")));

    // Combine our routes
    let routes = chat.or(files).or(index).or(redirect);

    // Start the server
    println!("Server started at http://localhost:3030");
    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

async fn user_connected(ws: warp::ws::WebSocket, users: Users, tx: broadcast::Sender<String>) {
    // Use a counter to assign a new, unique user ID
    let my_id = rand::random::<u64>().to_string();

    // Split the socket into a sender and receive of messages.
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();

    // Use an unbounded channel to handle buffering and flushing of messages
    // to the WebSocket...
    let (tx_unbounded, mut rx_unbounded) = mpsc::unbounded_channel();

    tokio::task::spawn(async move {
        while let Some(message) = rx_unbounded.recv().await {
            if let Err(e) = user_ws_tx.send(message).await {
                eprintln!("websocket send error: {}", e);
                break;
            }
        }
    });

    // Save the sender in our list of connected users.
    users.lock().await.insert(my_id.clone(), tx_unbounded);

    // Create a task that will receive broadcast messages and forward them to this client
    let mut rx = tx.subscribe();
    let my_id_clone = my_id.clone();
    let users_clone = users.clone();
    tokio::task::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if let Some(user_tx) = users_clone.lock().await.get(&my_id_clone) {
                let _ = user_tx.send(warp::ws::Message::text(msg));
            }
        }
    });

    println!("New chat user: {}", my_id);

    // Process incoming messages
    while let Some(result) = user_ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("websocket error(uid={}): {}", my_id, e);
                break;
            }
        };
        user_message(my_id.clone(), msg, &tx).await;
    }

    // user disconnected
    user_disconnected(my_id, &users).await;
}

async fn user_message(my_id: String, msg: warp::ws::Message, tx: &broadcast::Sender<String>) {
    // Skip any non-Text messages...
    let msg = if let Ok(s) = msg.to_str() {
        s
    } else {
        return;
    };

    let new_msg = format!("<User#{}>: {}", my_id, msg);
    let _ = tx.send(new_msg);
}

async fn user_disconnected(my_id: String, users: &Users) {
    println!("Good bye user: {}!", my_id);
    // Stream closed up, so remove from the user list
    users.lock().await.remove(&my_id);
}

