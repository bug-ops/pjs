//! E-commerce dataset generation for PJS demonstrations

use super::DatasetSize;
use rand::Rng;
use serde_json::{Value, json};

const PRODUCT_NAMES: &[&str] = &[
    "MacBook Pro",
    "iPhone",
    "AirPods",
    "iPad",
    "Apple Watch",
    "Dell XPS",
    "Surface Pro",
    "ThinkPad",
    "Galaxy S24",
    "Pixel 8",
    "Gaming Chair",
    "Mechanical Keyboard",
    "4K Monitor",
    "Webcam",
    "Headphones",
    "Coffee Maker",
    "Bluetooth Speaker",
    "Smart TV",
    "Tablet Stand",
    "Phone Case",
];

const CATEGORIES: &[&str] = &[
    "Electronics",
    "Computers",
    "Mobile",
    "Audio",
    "Gaming",
    "Home & Garden",
    "Sports",
    "Books",
    "Clothing",
    "Accessories",
];

const BRANDS: &[&str] = &[
    "Apple",
    "Samsung",
    "Google",
    "Microsoft",
    "Dell",
    "HP",
    "Lenovo",
    "Sony",
    "LG",
    "ASUS",
];

const COLORS: &[&str] = &[
    "Black",
    "White",
    "Silver",
    "Space Gray",
    "Gold",
    "Blue",
    "Red",
    "Green",
    "Purple",
    "Rose Gold",
];

/// Generate comprehensive e-commerce dataset
pub fn generate_ecommerce_data(size: DatasetSize) -> Value {
    let product_count = size.item_count(super::DatasetType::ECommerce);
    // TODO: Update to use rand::rng() instead of deprecated rand::rng()
    let mut rng = rand::rng();

    // Generate products with varying complexity based on size
    let products: Vec<Value> = (0..product_count)
        .map(|i| {
            let base_price = 50.0 + (i as f64 * 23.7) % 2000.0;
            let discount_percent = if i % 7 == 0 { rng.random_range(5..30) } else { 0 };
            let final_price = base_price * f64::from(100 - discount_percent) / 100.0;
            let mut product = json!({
                "id": 1000 + i,
                "name": format!("{} - {}", 
                    PRODUCT_NAMES[i % PRODUCT_NAMES.len()],
                    if i > PRODUCT_NAMES.len() { format!("Model {}", i / PRODUCT_NAMES.len()) } else { "Pro".to_string() }
                ),
                "sku": format!("SKU-{:06}", 1000 + i),
                "brand": BRANDS[i % BRANDS.len()],
                "category": CATEGORIES[i % CATEGORIES.len()],
                "price": {
                    "original": (base_price * 100.0).round() / 100.0,
                    "current": (final_price * 100.0).round() / 100.0,
                    "currency": "USD",
                    "discount_percent": discount_percent
                },
                "availability": {
                    "in_stock": i % 8 != 0,
                    "quantity": if i % 8 == 0 { 0 } else { rng.random_range(1..100) },
                    "warehouse": format!("WH-{}", (i % 5) + 1),
                    "estimated_delivery": if i % 8 == 0 { 
                        Value::Null
                    } else {
                        json!(format!("2024-01-{:02}", ((i % 28) + 1).min(28)))
                    }
                },
                "rating": {
                    "average": (f64::from(rng.random_range(30..50)) / 10.0).min(5.0),
                    "count": rng.random_range(5..500),
                    "distribution": {
                        "5": rng.random_range(20..60),
                        "4": rng.random_range(15..40),
                        "3": rng.random_range(5..25),
                        "2": rng.random_range(1..10),
                        "1": rng.random_range(0..5)
                    }
                },
                "images": {
                    "primary": format!("https://cdn.store.com/products/{}/main.webp", 1000 + i),
                    "gallery": (0..rng.random_range(2..6)).map(|j| 
                        format!("https://cdn.store.com/products/{}/img{}.webp", 1000 + i, j)
                    ).collect::<Vec<_>>(),
                    "thumbnail": format!("https://cdn.store.com/products/{}/thumb.webp", 1000 + i)
                }
            });

            // Add specifications based on category
            let specs = match CATEGORIES[i % CATEGORIES.len()] {
                "Electronics" | "Computers" => json!({
                    "processor": format!("{} Chip", if i % 3 == 0 { "M3" } else if i % 3 == 1 { "Intel i7" } else { "AMD Ryzen" }),
                    "memory": format!("{}GB", if i % 4 == 0 { "8" } else if i % 4 == 1 { "16" } else if i % 4 == 2 { "32" } else { "64" }),
                    "storage": format!("{}GB SSD", rng.random_range(256..2048)),
                    "display": format!("{:.1}\"", 13.0 + (i % 10) as f64 * 0.3),
                    "weight": format!("{:.1}kg", 1.2 + (i % 5) as f64 * 0.3)
                }),
                "Mobile" => json!({
                    "display": format!("{:.1}\"", 5.5 + (i % 8) as f64 * 0.2),
                    "storage": format!("{}GB", if i % 4 == 0 { "128" } else if i % 4 == 1 { "256" } else if i % 4 == 2 { "512" } else { "1024" }),
                    "camera": format!("{}MP", rng.random_range(12..108)),
                    "battery": format!("{}mAh", rng.random_range(3000..5000)),
                    "color": COLORS[i % COLORS.len()]
                }),
                _ => json!({
                    "dimensions": format!("{}x{}x{}cm", 
                        rng.random_range(10..50),
                        rng.random_range(10..50),
                        rng.random_range(5..30)
                    ),
                    "weight": format!("{:.1}kg", f64::from(rng.random_range(100..5000)) / 1000.0),
                    "material": "Premium Materials",
                    "warranty": format!("{} year{}", 
                        rng.random_range(1..4),
                        if rng.random_range(1..4) > 1 { "s" } else { "" }
                    )
                })
            };
            product["specifications"] = specs;

            // Add reviews for larger datasets
            if matches!(size, DatasetSize::Medium | DatasetSize::Large | DatasetSize::Huge) && i % 3 == 0 {
                let review_count = match size {
                    DatasetSize::Medium => rng.random_range(1..4),
                    DatasetSize::Large => rng.random_range(2..6),
                    DatasetSize::Huge => rng.random_range(3..8),
                    _ => 0,
                };
                let reviews: Vec<Value> = (0..review_count).map(|j| {
                    let rating = rng.random_range(1..6);
                    json!({
                        "id": format!("rev_{}_{}", 1000 + i, j),
                        "user": format!("user{}", (i * 7 + j) % 1000),
                        "rating": rating,
                        "title": format!("{} review for {}", 
                            match rating {
                                5 => "Excellent",
                                4 => "Great",
                                3 => "Good",
                                2 => "Fair",
                                _ => "Poor"
                            },
                            PRODUCT_NAMES[i % PRODUCT_NAMES.len()]
                        ),
                        "comment": generate_review_comment(rating),
                        "date": format!("2024-01-{:02}T{:02}:{:02}:00Z", 
                            rng.random_range(1..29),
                            rng.random_range(0..24),
                            rng.random_range(0..60)
                        ),
                        "verified_purchase": rng.random_bool(0.7),
                        "helpful_votes": rng.random_range(0..50)
                    })
                }).collect();
                product["reviews"] = json!(reviews);
            }

            product
        })
        .collect();

    // Generate store metadata
    let store_info = json!({
        "name": "PJS Demo Store",
        "description": "Interactive demonstration of Priority JSON Streaming",
        "version": "2.0",
        "status": "operational",
        "established": "2024-01-01",
        "currency": "USD",
        "shipping": {
            "free_threshold": 50.0,
            "standard_cost": 5.99,
            "expedited_cost": 12.99
        }
    });

    // Generate categories with counts
    let categories_with_counts: Vec<Value> = CATEGORIES
        .iter()
        .enumerate()
        .map(|(i, &category)| {
            let count = products
                .iter()
                .filter(|p| p["category"].as_str() == Some(category))
                .count();

            json!({
                "id": i + 1,
                "name": category,
                "product_count": count,
                "featured": i < 4, // First 4 categories are featured
                "icon": format!("icon-{}", category.to_lowercase().replace(' ', "-"))
            })
        })
        .collect();

    // Generate inventory summary
    let inventory = json!({
        "total_products": product_count,
        "in_stock": products.iter().filter(|p| p["availability"]["in_stock"].as_bool().unwrap_or(false)).count(),
        "out_of_stock": products.iter().filter(|p| !p["availability"]["in_stock"].as_bool().unwrap_or(true)).count(),
        "total_value": products.iter()
            .map(|p| p["price"]["current"].as_f64().unwrap_or(0.0))
            .sum::<f64>(),
        "avg_price": products.iter()
            .map(|p| p["price"]["current"].as_f64().unwrap_or(0.0))
            .sum::<f64>() / product_count as f64,
        "categories_count": CATEGORIES.len(),
        "brands_count": BRANDS.len()
    });

    // Generate business analytics
    let analytics = json!({
        "sales": {
            "today": {
                "revenue": rng.random_range(5000.0..25000.0),
                "orders": rng.random_range(50..300),
                "avg_order_value": rng.random_range(80.0..200.0)
            },
            "this_month": {
                "revenue": rng.random_range(150000.0..500000.0),
                "orders": rng.random_range(1500..8000),
                "growth_percent": rng.random_range(-5.0..25.0)
            }
        },
        "traffic": {
            "unique_visitors": rng.random_range(2000..15000),
            "page_views": rng.random_range(10000..75000),
            "bounce_rate": rng.random_range(25.0..45.0),
            "conversion_rate": rng.random_range(2.0..8.0)
        },
        "top_products": products.iter().take(5).cloned().collect::<Vec<_>>(),
        "performance": {
            "page_load_time_ms": rng.random_range(800..2500),
            "server_response_time_ms": rng.random_range(50..200),
            "uptime_percent": rng.random_range(99.0..99.99)
        }
    });

    // Combine everything
    json!({
        "store": store_info,
        "categories": categories_with_counts,
        "products": products,
        "inventory": inventory,
        "analytics": analytics,
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "dataset_info": {
            "type": "ecommerce",
            "size": format!("{size:?}").to_lowercase(),
            "product_count": product_count
        }
    })
}

/// Generate realistic review comments based on rating
fn generate_review_comment(rating: i32) -> String {
    let positive_comments = [
        "Excellent product! Highly recommended.",
        "Great value for money. Fast delivery.",
        "Perfect quality, exactly as described.",
        "Amazing features and great build quality.",
        "Love it! Will definitely buy again.",
    ];

    let neutral_comments = [
        "Good product overall, meets expectations.",
        "Decent quality for the price point.",
        "Works as expected, nothing special.",
        "Reasonable purchase, decent value.",
        "Okay product, does the job.",
    ];

    let negative_comments = [
        "Not worth the price, quality issues.",
        "Disappointing experience, poor build quality.",
        "Had issues with delivery and product quality.",
        "Expected better for this price range.",
        "Would not recommend, several problems.",
    ];

    // TODO: Update to use rand::rng() instead of deprecated rand::rng()
    let mut rng = rand::rng();

    match rating {
        5 | 4 => positive_comments[rng.random_range(0..positive_comments.len())].to_string(),
        3 => neutral_comments[rng.random_range(0..neutral_comments.len())].to_string(),
        _ => negative_comments[rng.random_range(0..negative_comments.len())].to_string(),
    }
}
