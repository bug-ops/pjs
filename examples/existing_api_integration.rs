//! Example: Integrating PJS into an existing API
//!
//! This example shows how to add PJS streaming capabilities to an existing
//! Axum API with minimal changes to the existing codebase.

use axum::{
    extract::{Path, Query},
    http::{header, HeaderMap},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::{collections::HashMap, net::SocketAddr};

// Import PJS extension
use pjson_rs::infrastructure::http::axum_extension::{
    PjsConfig, PjsExtension, PjsResponseExt
};

// Existing API models (unchanged)
#[derive(Debug, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
    email: String,
    profile: UserProfile,
    posts: Vec<Post>,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserProfile {
    bio: String,
    avatar_url: String,
    follower_count: u32,
    following_count: u32,
    settings: UserSettings,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserSettings {
    privacy: String,
    notifications: bool,
    theme: String,
    language: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Post {
    id: u64,
    title: String,
    content: String,
    created_at: String,
    likes: u32,
    comments: Vec<Comment>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Comment {
    id: u64,
    author: String,
    content: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct CreateUserRequest {
    name: String,
    email: String,
    bio: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::init();

    // Create existing API routes (no changes required)
    let existing_api = Router::new()
        .route("/users", get(list_users).post(create_user))
        .route("/users/:id", get(get_user))
        .route("/users/:id/posts", get(get_user_posts))
        .route("/posts/:id", get(get_post_with_comments))
        .route("/feed", get(get_social_feed))
        .route("/dashboard", get(get_analytics_dashboard));

    // Configure PJS extension
    let pjs_config = PjsConfig {
        route_prefix: "/stream".to_string(), // Custom prefix
        auto_detect: true, // Enable auto-detection via headers
        ..Default::default()
    };

    // Add PJS capabilities with one line!
    let pjs_extension = PjsExtension::new(pjs_config);
    let app = pjs_extension.extend_router(existing_api);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("🚀 Enhanced API with PJS streaming on http://{}", addr);
    println!();
    println!("📋 Regular API endpoints (unchanged):");
    println!("   GET  http://{}/users", addr);
    println!("   GET  http://{}/users/1", addr);
    println!("   GET  http://{}/feed", addr);
    println!("   GET  http://{}/dashboard", addr);
    println!();
    println!("⚡ PJS streaming endpoints (auto-added):");
    println!("   GET  http://{}/stream/health", addr);
    println!("   POST http://{}/stream/stream", addr);
    println!();
    println!("🔄 To enable streaming, add headers:");
    println!("   Accept: text/event-stream (for SSE)");
    println!("   Accept: application/pjs-stream (for PJS)");
    println!("   X-PJS-Stream: true (explicit flag)");
    println!();
    println!("💡 Examples:");
    println!("   # Regular response");
    println!("   curl http://{}/users/1", addr);
    println!();
    println!("   # Streaming response");
    println!("   curl -H 'Accept: text/event-stream' http://{}/users/1", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// Existing API handlers (no changes required)
async fn list_users() -> Json<JsonValue> {
    Json(json!({
        "users": [
            {
                "id": 1,
                "name": "Alice Johnson", 
                "email": "alice@example.com",
                "profile": {
                    "bio": "Software engineer passionate about Rust and web technologies",
                    "avatar_url": "https://api.dicebear.com/7.x/avataaars/svg?seed=Alice",
                    "follower_count": 1247,
                    "following_count": 89,
                    "settings": {
                        "privacy": "public",
                        "notifications": true,
                        "theme": "dark",
                        "language": "en"
                    }
                },
                "posts": generate_posts(1, 5)
            },
            {
                "id": 2,
                "name": "Bob Smith",
                "email": "bob@example.com", 
                "profile": {
                    "bio": "Tech enthusiast and coffee lover. Building the future one commit at a time.",
                    "avatar_url": "https://api.dicebear.com/7.x/avataaars/svg?seed=Bob",
                    "follower_count": 892,
                    "following_count": 156,
                    "settings": {
                        "privacy": "public",
                        "notifications": false,
                        "theme": "light",
                        "language": "en"
                    }
                },
                "posts": generate_posts(2, 3)
            }
        ],
        "total": 2,
        "page": 1,
        "per_page": 20
    }))
}

async fn get_user(Path(user_id): Path<u64>) -> Json<JsonValue> {
    Json(json!({
        "id": user_id,
        "name": format!("User {}", user_id),
        "email": format!("user{}@example.com", user_id),
        "profile": {
            "bio": format!("This is user {} with lots of interesting content and a very detailed profile that takes time to load", user_id),
            "avatar_url": format!("https://api.dicebear.com/7.x/avataaars/svg?seed=User{}", user_id),
            "follower_count": (user_id * 123) % 5000,
            "following_count": (user_id * 47) % 500,
            "settings": {
                "privacy": if user_id % 2 == 0 { "public" } else { "private" },
                "notifications": user_id % 3 == 0,
                "theme": if user_id % 2 == 0 { "dark" } else { "light" },
                "language": "en"
            }
        },
        "posts": generate_posts(user_id, 8),
        "activity": {
            "last_login": "2024-01-15T10:30:00Z",
            "post_count": 23,
            "comment_count": 145,
            "like_count": 67
        }
    }))
}

async fn create_user(Json(request): Json<CreateUserRequest>) -> Json<JsonValue> {
    Json(json!({
        "id": 999,
        "name": request.name,
        "email": request.email,
        "profile": {
            "bio": request.bio.unwrap_or("New user".to_string()),
            "avatar_url": "https://api.dicebear.com/7.x/avataaars/svg?seed=NewUser",
            "follower_count": 0,
            "following_count": 0,
            "settings": {
                "privacy": "public",
                "notifications": true,
                "theme": "light", 
                "language": "en"
            }
        },
        "posts": [],
        "created_at": "2024-01-15T12:00:00Z"
    }))
}

async fn get_user_posts(
    Path(user_id): Path<u64>,
    Query(params): Query<HashMap<String, String>>
) -> Json<JsonValue> {
    let limit = params.get("limit")
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(10);
    
    Json(json!({
        "user_id": user_id,
        "posts": generate_posts(user_id, limit),
        "total": 23,
        "has_more": limit < 23
    }))
}

async fn get_post_with_comments(Path(post_id): Path<u64>) -> Json<JsonValue> {
    Json(json!({
        "id": post_id,
        "title": format!("Amazing Post #{}", post_id),
        "content": format!("This is the content of post {} with lots of interesting details and insights that users love to read. It contains multiple paragraphs and rich formatting.", post_id),
        "author": {
            "id": (post_id % 10) + 1,
            "name": format!("Author {}", (post_id % 10) + 1),
            "avatar_url": format!("https://api.dicebear.com/7.x/avataaars/svg?seed=Author{}", post_id % 10)
        },
        "created_at": "2024-01-14T15:30:00Z",
        "likes": post_id * 7,
        "comments": generate_comments(post_id, 12),
        "metadata": {
            "reading_time": "3 minutes",
            "word_count": 450,
            "tags": ["technology", "programming", "rust", "web-development"],
            "category": "engineering"
        }
    }))
}

async fn get_social_feed() -> Json<JsonValue> {
    Json(json!({
        "feed": {
            "posts": generate_posts(0, 20),
            "stories": [
                {"id": 1, "author": "Alice", "content": "Working on exciting new features!"},
                {"id": 2, "author": "Bob", "content": "Beautiful sunset today 🌅"},
                {"id": 3, "author": "Charlie", "content": "New blog post about Rust performance"},
            ],
            "trending": [
                {"topic": "Rust Programming", "count": 1247},
                {"topic": "Web Development", "count": 892},
                {"topic": "API Design", "count": 567}
            ],
            "recommendations": generate_posts(100, 5)
        },
        "pagination": {
            "current_page": 1,
            "per_page": 20,
            "total_pages": 15,
            "has_next": true
        },
        "metadata": {
            "generated_at": "2024-01-15T12:00:00Z",
            "user_timezone": "UTC",
            "feed_algorithm": "chronological_with_boost"
        }
    }))
}

async fn get_analytics_dashboard() -> Json<JsonValue> {
    Json(json!({
        "dashboard": {
            "key_metrics": {
                "total_users": 15847,
                "active_users_24h": 3241,
                "total_posts": 89234,
                "posts_today": 432,
                "engagement_rate": 0.067
            },
            "charts": {
                "user_growth": (1..=30).map(|day| json!({
                    "date": format!("2024-01-{:02}", day),
                    "new_users": (day * 23 + 100) % 200,
                    "active_users": (day * 67 + 2000) % 1000 + 2000
                })).collect::<Vec<_>>(),
                "content_metrics": (1..=24).map(|hour| json!({
                    "hour": hour,
                    "posts": (hour * 13) % 50,
                    "comments": (hour * 27) % 100,
                    "likes": (hour * 41) % 300
                })).collect::<Vec<_>>()
            },
            "top_content": generate_posts(200, 10),
            "user_segments": [
                {"segment": "New Users", "count": 2341, "growth": 12.5},
                {"segment": "Active Users", "count": 8923, "growth": 5.2},
                {"segment": "Power Users", "count": 1247, "growth": 8.9}
            ]
        },
        "metadata": {
            "report_generated": "2024-01-15T12:00:00Z",
            "data_freshness": "5 minutes",
            "next_update": "2024-01-15T12:05:00Z"
        }
    }))
}

// Helper functions
fn generate_posts(base_id: u64, count: usize) -> Vec<JsonValue> {
    (0..count).map(|i| {
        let post_id = base_id * 100 + i as u64;
        json!({
            "id": post_id,
            "title": format!("Interesting Post #{}", post_id),
            "content": format!("This is post {} content with detailed information and engaging text that users will find valuable.", post_id),
            "author": format!("Author {}", (post_id % 10) + 1),
            "created_at": "2024-01-14T10:30:00Z",
            "likes": post_id * 3,
            "comments": generate_comments(post_id, 3)
        })
    }).collect()
}

fn generate_comments(post_id: u64, count: usize) -> Vec<JsonValue> {
    (0..count).map(|i| {
        json!({
            "id": post_id * 1000 + i as u64,
            "author": format!("Commenter {}", i + 1),
            "content": format!("Great post! Comment #{} on post {}", i + 1, post_id),
            "created_at": "2024-01-14T11:45:00Z",
            "likes": (i * 7) % 20
        })
    }).collect()
}