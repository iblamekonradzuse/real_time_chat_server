use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::fs;
use warp::Filter;
use rand::Rng;
use sha2::{Sha256, Digest};

// Our global state
type Users = Arc<Mutex<HashMap<String, (mpsc::UnboundedSender<String>, String)>>>;
type RegisteredUsers = Arc<Mutex<HashMap<String, User>>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct User {
    username: String,
    password_hash: String,
}

#[derive(Debug, Deserialize)]
struct LoginInfo {
    username: String,
    password: String,
}

#[tokio::main]
async fn main() {
    // Create our global state
    let users = Users::default();
    let registered_users = RegisteredUsers::default();

    // Load registered users from JSON file
    load_registered_users(&registered_users).await;

    // Create a channel for broadcasting messages to all connected clients
    let (tx, _rx) = broadcast::channel(100);

    // Define our WebSocket route
    let chat = warp::path("chat")
        .and(warp::ws())
        .and(warp::any().map(move || users.clone()))
        .and(warp::any().map(move || tx.clone()))
        .and(warp::query::<LoginInfo>())
        .map(|ws: warp::ws::Ws, users, tx, login_info: LoginInfo| {
            ws.on_upgrade(move |socket| user_connected(socket, users, tx, login_info))
        });

    // Define our registration route
    let register_users = registered_users.clone();
    let register = warp::path("register")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(move || register_users.clone()))
        .and_then(register_user);

    // Define our login route
    let login_users = registered_users.clone();
    let login = warp::path("login")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(move || login_users.clone()))
        .and_then(login_user);

    // Serve static files
    let files = warp::path("static")
        .and(warp::fs::dir("static"));

    // Serve index.html at the root
    let index = warp::path::end()
        .and(warp::fs::file("static/index.html"));

    // Combine our routes
    let routes = chat
        .or(register)
        .or(login)
        .or(files)
        .or(index);

    // Start the server
    println!("Server started at http://localhost:3030");
    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

async fn load_registered_users(registered_users: &RegisteredUsers) {
    match fs::read_to_string("users.json").await {
        Ok(contents) => {
            let users: HashMap<String, User> = serde_json::from_str(&contents).unwrap_or_default();
            let mut lock = registered_users.lock().await;
            *lock = users;
        }
        Err(_) => println!("No existing users file found. Starting with an empty user list."),
    }
}

async fn save_registered_users(registered_users: &RegisteredUsers) {
    let lock = registered_users.lock().await;
    let json = serde_json::to_string(&*lock).unwrap();
    if let Err(e) = fs::write("users.json", json).await {
        eprintln!("Failed to save users: {}", e);
    }
}


async fn register_user(
    user: User,
    registered_users: RegisteredUsers,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut lock = registered_users.lock().await;

    if lock.contains_key(&user.username) {
        Ok(warp::reply::json(&serde_json::json!({"status": "error", "message": "Username already exists"})))
    } else {
        // Hash the password before storing
        let mut hasher = Sha256::new();
        hasher.update(user.password_hash);
        let password_hash = format!("{:x}", hasher.finalize());

        let new_user = User {
            username: user.username.clone(),
            password_hash,
        };

        lock.insert(user.username.clone(), new_user);
        drop(lock);
        save_registered_users(&registered_users).await;
        Ok(warp::reply::json(&serde_json::json!({"status": "success", "message": "User registered successfully"})))
    }
}

async fn login_user(
    user: LoginInfo,
    registered_users: RegisteredUsers,
) -> Result<impl warp::Reply, warp::Rejection> {
    let lock = registered_users.lock().await;

    if let Some(registered_user) = lock.get(&user.username) {
        // Hash the provided password and compare
        let mut hasher = Sha256::new();
        hasher.update(&user.password);
        let password_hash = format!("{:x}", hasher.finalize());

        if registered_user.password_hash == password_hash {
            Ok(warp::reply::json(&serde_json::json!({"status": "success", "message": "Login successful"})))
        } else {
            Ok(warp::reply::json(&serde_json::json!({"status": "error", "message": "Invalid password"})))
        }
    } else {
        Ok(warp::reply::json(&serde_json::json!({"status": "error", "message": "User not found"})))
    }
}

async fn user_connected(ws: warp::ws::WebSocket, users: Users, tx: broadcast::Sender<String>, login_info: LoginInfo) {
    let my_id = rand::thread_rng().gen::<u64>().to_string();
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();
    let (tx_unbounded, mut rx_unbounded) = mpsc::unbounded_channel();

    tokio::task::spawn(async move {
        while let Some(message) = rx_unbounded.recv().await {
            if let Err(e) = user_ws_tx.send(warp::ws::Message::text(message)).await {
                eprintln!("websocket send error: {}", e);
                break;
            }
        }
    });

    users.lock().await.insert(my_id.clone(), (tx_unbounded, login_info.username.clone()));

    let mut rx = tx.subscribe();
    let my_id_clone = my_id.clone();
    let users_clone = users.clone();
    tokio::task::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if let Some((user_tx, _)) = users_clone.lock().await.get(&my_id_clone) {
                let _ = user_tx.send(msg);
            }
        }
    });

    println!("New chat user: {} ({})", login_info.username, my_id);

    while let Some(result) = user_ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("websocket error(uid={}): {}", my_id, e);
                break;
            }
        };
        user_message(my_id.clone(), msg, &users, &tx).await;
    }

    user_disconnected(my_id, &users).await;
}

async fn user_message(my_id: String, msg: warp::ws::Message, users: &Users, tx: &broadcast::Sender<String>) {
    let msg = if let Ok(s) = msg.to_str() {
        s
    } else {
        return;
    };

    let username = users.lock().await.get(&my_id).map(|(_, name)| name.clone()).unwrap_or_else(|| "Unknown".to_string());
    let new_msg = format!("{}: {}", username, msg);  // Changed from "<{}>: {}" to "{}: {}"
    let _ = tx.send(new_msg);
}

async fn user_disconnected(my_id: String, users: &Users) {
    let username = users.lock().await.get(&my_id).map(|(_, name)| name.clone()).unwrap_or_else(|| "Unknown".to_string());
    println!("Good bye user: {} ({})", username, my_id);
    users.lock().await.remove(&my_id);
}

