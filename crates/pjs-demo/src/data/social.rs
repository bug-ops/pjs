//! Social media dataset generation for PJS demonstrations

use super::DatasetSize;
use serde_json::{json, Value};
use rand::Rng;

const USERNAMES: &[&str] = &[
    "alextech", "sarahdev", "mikecoder", "jennyui", "tomstack",
    "lisadata", "chrisml", "emilyai", "davidweb", "annacloud",
];

const POST_TYPES: &[&str] = &[
    "text", "image", "video", "link", "poll", "event",
];

const HASHTAGS: &[&str] = &[
    "#technology", "#coding", "#ai", "#webdev", "#mobile",
    "#design", "#startup", "#innovation", "#programming", "#data",
];

/// Generate social media feed dataset
pub fn generate_social_data(size: DatasetSize) -> Value {
    let post_count = size.item_count(super::DatasetType::SocialMedia);
    let mut rng = rand::rng();
    
    // Generate users
    let users: Vec<Value> = USERNAMES.iter().enumerate().map(|(i, &username)| {
        // Fixed: Extract complex expressions outside json! macro
        let locations = ["San Francisco", "New York", "London", "Berlin", "Tokyo"];
        let user_location = locations[i % 5];
        
        json!({
            "id": i + 1,
            "username": username,
            "display_name": format!("{} {}", 
                ["Alex", "Sarah", "Mike", "Jenny", "Tom", "Lisa", "Chris", "Emily", "David", "Anna"][i],
                ["Smith", "Johnson", "Williams", "Brown", "Jones", "Garcia", "Miller", "Davis", "Rodriguez", "Martinez"][i]
            ),
            "avatar": format!("https://cdn.social.com/avatars/{}.jpg", username),
            "verified": i < 3, // First 3 users are verified
            "followers": rng.random_range(100..100000),
            "following": rng.random_range(50..5000),
            "bio": "Software engineer passionate about technology and innovation. Building the future one line of code at a time.",
            "location": user_location,
            "joined_date": format!("202{}-{:02}-{:02}", 
                rng.random_range(0..4), 
                rng.random_range(1..13), 
                rng.random_range(1..29)
            )
        })
    }).collect();

    // Generate posts
    let posts: Vec<Value> = (0..post_count).map(|i| {
        let user_id = (i % USERNAMES.len()) + 1;
        let post_type = POST_TYPES[i % POST_TYPES.len()];
        let likes = rng.random_range(0..1000);
        let shares = rng.random_range(0..likes/3);
        let comments_count = rng.random_range(0..50);
        
        let mut post = json!({
            "id": format!("post_{}", i + 1),
            "user_id": user_id,
            "type": post_type,
            "created_at": format!("2024-01-{:02}T{:02}:{:02}:00Z", 
                rng.random_range(1..29), 
                rng.random_range(0..24),
                rng.random_range(0..60)
            ),
            "content": {
                "text": generate_post_content(post_type, i),
                "hashtags": (0..rng.random_range(1..4)).map(|_| 
                    HASHTAGS[rng.random_range(0..HASHTAGS.len())]
                ).collect::<Vec<_>>(),
                "mentions": if rng.random_bool(0.3) { 
                    vec![format!("@{}", USERNAMES[rng.random_range(0..USERNAMES.len())])]
                } else { 
                    vec![] 
                }
            },
            "engagement": {
                "likes": likes,
                "shares": shares,
                "comments": comments_count,
                "views": rng.random_range(likes..likes*10)
            },
            "location": if rng.random_bool(0.4) {
                Some(["San Francisco, CA", "New York, NY", "London, UK", "Berlin, Germany", "Tokyo, Japan"][rng.random_range(0..5)])
            } else {
                None
            }
        });

        // Add media based on post type
        match post_type {
            "image" => {
                post["media"] = json!({
                    "type": "image",
                    "url": format!("https://cdn.social.com/images/post_{}.jpg", i + 1),
                    "thumbnail": format!("https://cdn.social.com/thumbs/post_{}_thumb.jpg", i + 1),
                    "alt_text": "Shared image",
                    "dimensions": {
                        "width": rng.random_range(800..1920),
                        "height": rng.random_range(600..1080)
                    }
                });
            }
            "video" => {
                post["media"] = json!({
                    "type": "video",
                    "url": format!("https://cdn.social.com/videos/post_{}.mp4", i + 1),
                    "thumbnail": format!("https://cdn.social.com/thumbs/post_{}_thumb.jpg", i + 1),
                    "duration_seconds": rng.random_range(15..300),
                    "dimensions": {
                        "width": rng.random_range(720..1920),
                        "height": rng.random_range(480..1080)
                    }
                });
            }
            "link" => {
                post["media"] = json!({
                    "type": "link",
                    "url": format!("https://example.com/article/{}", i + 1),
                    "title": format!("Interesting Article About Technology #{}", i + 1),
                    "description": "A comprehensive look at the latest trends in software development and emerging technologies.",
                    "image": format!("https://cdn.social.com/previews/link_{}.jpg", i + 1),
                    "domain": "example.com"
                });
            }
            _ => {}
        }

        // Add comments for larger datasets
        if matches!(size, DatasetSize::Large | DatasetSize::Huge) && comments_count > 0 {
            let comments: Vec<Value> = (0..comments_count.min(10)).map(|j| {
                let commenter_id = rng.random_range(1..=USERNAMES.len());
                json!({
                    "id": format!("comment_{}_{}", i + 1, j + 1),
                    "user_id": commenter_id,
                    "content": generate_comment_content(),
                    "created_at": format!("2024-01-{:02}T{:02}:{:02}:00Z", 
                        rng.random_range(1..29), 
                        rng.random_range(0..24),
                        rng.random_range(0..60)
                    ),
                    "likes": rng.random_range(0..20),
                    "replies": if rng.random_bool(0.2) { rng.random_range(1..5) } else { 0 }
                })
            }).collect();
            
            post["comments"] = json!(comments);
        }

        post
    }).collect();

    // Generate trending topics
    let trending: Vec<Value> = HASHTAGS.iter().take(10).enumerate().map(|(i, &hashtag)| {
        // Fixed: Extract complex expressions outside json! macro
        let categories = ["Technology", "Programming", "Innovation", "Design", "Startup"];
        let category = categories[i % 5];
        
        json!({
            "hashtag": hashtag,
            "posts_count": rng.random_range(100..10000),
            "trend_score": 100 - i * 10,
            "category": category
        })
    }).collect();

    // Generate feed analytics
    let analytics = json!({
        "engagement": {
            "total_likes": posts.iter().map(|p| p["engagement"]["likes"].as_u64().unwrap_or(0)).sum::<u64>(),
            "total_shares": posts.iter().map(|p| p["engagement"]["shares"].as_u64().unwrap_or(0)).sum::<u64>(),
            "total_comments": posts.iter().map(|p| p["engagement"]["comments"].as_u64().unwrap_or(0)).sum::<u64>(),
            "engagement_rate": rng.random_range(3.0..8.0)
        },
        "reach": {
            "impressions": rng.random_range(50000..500000),
            "unique_users": rng.random_range(10000..100000),
            "viral_coefficient": rng.random_range(1.1..2.5)
        },
        "demographics": {
            "age_groups": {
                "18-24": 25,
                "25-34": 35,
                "35-44": 25,
                "45-54": 10,
                "55+": 5
            },
            "top_locations": ["San Francisco", "New York", "London", "Berlin", "Tokyo"]
        }
    });

    json!({
        "feed": {
            "name": "PJS Social Demo",
            "description": "Interactive social media feed demonstration",
            "version": "1.0"
        },
        "users": users,
        "posts": posts,
        "trending": trending,
        "analytics": analytics,
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "dataset_info": {
            "type": "social",
            "size": format!("{:?}", size).to_lowercase(),
            "post_count": post_count,
            "user_count": users.len()
        }
    })
}

fn generate_post_content(post_type: &str, index: usize) -> String {
    match post_type {
        "text" => {
            let texts = [
                "Just finished implementing a new feature using Rust! The performance improvements are incredible. #coding #rust",
                "Working on an AI project that processes streaming JSON data in real-time. The future is now! #ai #streaming",
                "Hot take: Progressive web apps are the future of mobile development. Who else agrees? #webdev #mobile",
                "Debugging session lasted 6 hours, but finally found the issue. One missing semicolon! #programming #debugging",
                "Excited to share my latest open source project with the community. Check it out! #opensource #technology"
            ];
            texts[index % texts.len()].to_string()
        }
        "image" => "Check out this amazing sunset from my coding setup! Perfect evening for some productive programming.".to_string(),
        "video" => "Quick tutorial on optimizing JSON parsing performance. Link in bio for full details!".to_string(),
        "link" => "Found this incredible article about the future of streaming protocols. Definitely worth a read!".to_string(),
        "poll" => "Which programming language will dominate in 2025? Cast your vote below!".to_string(),
        "event" => "Hosting a virtual meetup next week about real-time data streaming. Who's interested?".to_string(),
        _ => "Sharing some thoughts about technology and innovation in software development.".to_string(),
    }
}

fn generate_comment_content() -> String {
    let comments = [
        "Great post! Thanks for sharing.",
        "This is exactly what I was looking for.",
        "Interesting perspective, never thought about it that way.",
        "Can you share more details about this?",
        "Completely agree with your points here.",
        "This helped me solve a similar problem!",
        "Looking forward to seeing more content like this.",
        "Thanks for the insights, very helpful!",
    ];
    
    let mut rng = rand::rng();
    comments[rng.random_range(0..comments.len())].to_string()
}