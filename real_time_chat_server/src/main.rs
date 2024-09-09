use futures::{SinkExt, StreamExt};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{broadcast, mpsc, Mutex};
use warp::http::{Response, StatusCode};
use warp::hyper::Body;
use warp::Filter;

type Users = Arc<Mutex<HashMap<String, (mpsc::UnboundedSender<String>, String)>>>;
type RegisteredUsers = Arc<Mutex<HashMap<String, User>>>;
type Messages = Arc<Mutex<HashMap<String, Message>>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct User {
    username: String,
    password_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Message {
    id: String,
    username: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct LoginInfo {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct MessageAction {
    #[serde(rename = "type")]
    action_type: String,
    id: Option<String>,
    content: Option<String>,
}

#[tokio::main]
async fn main() {
    let users = Users::default();
    let registered_users = RegisteredUsers::default();
    let messages = Messages::default();

    load_registered_users(&registered_users).await;

    let (tx, _rx) = broadcast::channel(100);

    let register_users = registered_users.clone();
    let login_users = registered_users.clone();

    let chat = warp::path("chat")
        .and(warp::ws())
        .and(warp::any().map(move || users.clone()))
        .and(warp::any().map(move || tx.clone()))
        .and(warp::any().map(move || messages.clone()))
        .and(warp::query::<LoginInfo>())
        .map(
            |ws: warp::ws::Ws, users, tx, messages, login_info: LoginInfo| {
                println!("New WebSocket connection attempt"); // Debug print
                ws.on_upgrade(move |socket| user_connected(socket, users, tx, messages, login_info))
            },
        );

    let register = warp::path("register")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(move || register_users.clone()))
        .and_then(register_user);

    let login = warp::path("login")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(move || login_users.clone()))
        .and_then(login_user);

    let files = warp::path("static").and(warp::fs::dir("static"));
    let index = warp::path::end().and(warp::fs::file("static/index.html"));

    let routes = chat.or(register).or(login).or(files).or(index);

    println!("Server started at http://localhost:3030");
    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

async fn load_registered_users(registered_users: &RegisteredUsers) {
    match fs::read_to_string("users.json").await {
        Ok(contents) => {
            let users: HashMap<String, User> = serde_json::from_str(&contents).unwrap_or_default();
            let mut lock = registered_users.lock().await;
            *lock = users;
            println!("Loaded {} registered users", lock.len()); // Debug print
        }
        Err(_) => println!("No existing users file found. Starting with an empty user list."),
    }
}

async fn save_registered_users(registered_users: &RegisteredUsers) {
    let lock = registered_users.lock().await;
    let json = serde_json::to_string(&*lock).unwrap();
    if let Err(e) = fs::write("users.json", json).await {
        eprintln!("Failed to save users: {}", e);
    } else {
        println!("Saved {} registered users", lock.len()); // Debug print
    }
}

async fn register_user(
    user: User,
    registered_users: RegisteredUsers,
) -> Result<impl warp::Reply, warp::Rejection> {
    println!("Attempting to register user: {}", user.username); // Debug print
    let mut lock = registered_users.lock().await;

    if lock.contains_key(&user.username) {
        println!("Registration failed: Username already exists"); // Debug print
        Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "status": "error",
                "message": "Username already exists"
            })),
            warp::http::StatusCode::BAD_REQUEST,
        ))
    } else {
        let mut hasher = Sha256::new();
        hasher.update(&user.password_hash);
        let password_hash = format!("{:x}", hasher.finalize());

        let new_user = User {
            username: user.username.clone(),
            password_hash,
        };

        lock.insert(user.username.clone(), new_user);
        drop(lock);
        save_registered_users(&registered_users).await;

        println!("User registered successfully: {}", user.username); // Debug print
        Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "status": "success",
                "message": "User registered successfully"
            })),
            warp::http::StatusCode::OK,
        ))
    }
}

fn create_json_response(status: StatusCode, body: serde_json::Value) -> Response<Body> {
    let json = serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string());
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap()
}

async fn login_user(
    user: LoginInfo,
    registered_users: RegisteredUsers,
) -> Result<impl warp::Reply, warp::Rejection> {
    println!("Login attempt for user: {}", user.username); // Debug print
    let lock = registered_users.lock().await;

    if let Some(registered_user) = lock.get(&user.username) {
        let mut hasher = Sha256::new();
        hasher.update(&user.password);
        let password_hash = format!("{:x}", hasher.finalize());

        if registered_user.password_hash == password_hash {
            println!("Login successful for user: {}", user.username); // Debug print
            Ok(create_json_response(
                StatusCode::OK,
                json!({
                    "status": "success",
                    "message": "Login successful"
                }),
            ))
        } else {
            println!("Login failed: Invalid password for user: {}", user.username); // Debug print
            Ok(create_json_response(
                StatusCode::UNAUTHORIZED,
                json!({
                    "status": "error",
                    "message": "Invalid password"
                }),
            ))
        }
    } else {
        println!("Login failed: User not found: {}", user.username); // Debug print
        Ok(create_json_response(
            StatusCode::NOT_FOUND,
            json!({
                "status": "error",
                "message": "User not found"
            }),
        ))
    }
}

async fn user_connected(
    ws: warp::ws::WebSocket,
    users: Users,
    tx: broadcast::Sender<String>,
    messages: Messages,
    login_info: LoginInfo,
) {
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

    users
        .lock()
        .await
        .insert(my_id.clone(), (tx_unbounded, login_info.username.clone()));

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

    println!(
        "New chat user connected: {} ({})",
        login_info.username, my_id
    );

    while let Some(result) = user_ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("websocket error(uid={}): {}", my_id, e);
                break;
            }
        };
        user_message(
            my_id.clone(),
            msg,
            &users,
            &tx,
            &messages,
            &login_info.username,
        )
        .await;
    }

    user_disconnected(my_id, &users).await;
}

async fn user_message(
    my_id: String,
    msg: warp::ws::Message,
    users: &Users,
    tx: &broadcast::Sender<String>,
    messages: &Messages,
    username: &str,
) {
    let msg = if let Ok(s) = msg.to_str() {
        s
    } else {
        eprintln!("Failed to convert message to string");
        return;
    };

    println!("Received raw message from {}: {}", username, msg);

    let message_action: MessageAction = match serde_json::from_str(msg) {
        Ok(action) => {
            println!("Successfully parsed message action: {:?}", action);
            action
        }
        Err(e) => {
            eprintln!(
                "Failed to parse message action: {}. Raw message: {}",
                e, msg
            );
            return;
        }
    };

    match message_action.action_type.as_str() {
        "message" => {
            if let Some(content) = message_action.content {
                let message_id = rand::thread_rng().gen::<u64>().to_string();
                let message = Message {
                    id: message_id.clone(),
                    username: username.to_string(),
                    content: content.clone(),
                };
                messages.lock().await.insert(message_id.clone(), message);
                let new_msg = json!({
                    "type": "message",
                    "id": message_id,
                    "username": username,
                    "content": content
                });
                let json_msg = serde_json::to_string(&new_msg).unwrap();
                println!("Broadcasting message: {}", json_msg);
                let _ = tx.send(json_msg);
            }
        }
        "edit" => {
            if let (Some(id), Some(content)) = (message_action.id, message_action.content) {
                if let Some(message) = messages.lock().await.get_mut(&id) {
                    if message.username == username {
                        message.content = content.clone();
                        let edit_msg = json!({
                            "type": "edit",
                            "id": id,
                            "content": content
                        });
                        let json_msg = serde_json::to_string(&edit_msg).unwrap();
                        println!("Broadcasting edit: {}", json_msg);
                        let _ = tx.send(json_msg);
                    }
                }
            }
        }
        "delete" => {
            if let Some(id) = message_action.id {
                let mut messages_lock = messages.lock().await;
                if let Some(message) = messages_lock.get(&id) {
                    if message.username == username {
                        messages_lock.remove(&id);
                        let delete_msg = json!({
                            "type": "delete",
                            "id": id
                        });
                        let json_msg = serde_json::to_string(&delete_msg).unwrap();
                        println!("Broadcasting delete: {}", json_msg);
                        let _ = tx.send(json_msg);
                    }
                }
            }
        }
        _ => {
            println!("Unknown message type: {}", message_action.action_type);
        }
    }
}

async fn user_disconnected(my_id: String, users: &Users) {
    let username = users
        .lock()
        .await
        .get(&my_id)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    println!("User disconnected: {} ({})", username, my_id);
    users.lock().await.remove(&my_id);
}

