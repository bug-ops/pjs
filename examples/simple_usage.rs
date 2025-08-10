//! Simple usage example for SJSP

use sjsp_core::{Parser, ParseConfig, SemanticType};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create parser with default config
    let parser = Parser::new();

    // Example 1: Simple JSON object
    println!("=== Example 1: Simple Object ===");
    let json_obj = r#"{"name": "John", "age": 30, "city": "New York"}"#;
    let frame = parser.parse(json_obj.as_bytes())?;
    
    println!("Parsed frame with {} bytes", frame.payload.len());
    if let Some(semantics) = &frame.semantics {
        println!("Detected semantic type: {:?}", semantics.semantic_type);
    }

    // Example 2: Numeric array (should be detected as NumericArray)
    println!("\n=== Example 2: Numeric Array ===");
    let numeric_json = "[1.5, 2.7, 3.14, 4.2, 5.9]";
    let frame = parser.parse(numeric_json.as_bytes())?;
    
    if let Some(semantics) = &frame.semantics {
        println!("Detected semantic type: {:?}", semantics.semantic_type);
        match &semantics.semantic_type {
            SemanticType::NumericArray { dtype, length } => {
                println!("  Data type: {:?}", dtype);
                println!("  Length: {:?}", length);
            }
            _ => println!("  Not detected as numeric array"),
        }
    }

    // Example 3: Time series data
    println!("\n=== Example 3: Time Series ===");
    let timeseries_json = r#"[
        {"timestamp": "2024-01-01T00:00:00Z", "temperature": 22.5, "humidity": 65.2},
        {"timestamp": "2024-01-01T01:00:00Z", "temperature": 21.8, "humidity": 67.1},
        {"timestamp": "2024-01-01T02:00:00Z", "temperature": 20.9, "humidity": 69.3}
    ]"#;
    
    let frame = parser.parse(timeseries_json.as_bytes())?;
    if let Some(semantics) = &frame.semantics {
        println!("Detected semantic type: {:?}", semantics.semantic_type);
        match &semantics.semantic_type {
            SemanticType::TimeSeries { timestamp_field, value_fields, .. } => {
                println!("  Timestamp field: {}", timestamp_field);
                println!("  Value fields: {:?}", value_fields);
            }
            _ => println!("  Not detected as time series"),
        }
    }

    // Example 4: Tabular data
    println!("\n=== Example 4: Tabular Data ===");
    let table_json = r#"[
        {"id": 1, "name": "Alice", "score": 95.5, "active": true},
        {"id": 2, "name": "Bob", "score": 87.2, "active": false},
        {"id": 3, "name": "Carol", "score": 91.8, "active": true}
    ]"#;
    
    let frame = parser.parse(table_json.as_bytes())?;
    if let Some(semantics) = &frame.semantics {
        println!("Detected semantic type: {:?}", semantics.semantic_type);
        match &semantics.semantic_type {
            SemanticType::Table { columns, row_count } => {
                println!("  Columns: {}", columns.len());
                for col in columns {
                    println!("    {}: {:?}", col.name, col.dtype);
                }
                println!("  Row count: {:?}", row_count);
            }
            _ => println!("  Not detected as table"),
        }
    }

    // Example 5: GeoJSON
    println!("\n=== Example 5: GeoJSON ===");
    let geo_json = r#"{"type": "Point", "coordinates": [125.6, 10.1]}"#;
    let frame = parser.parse(geo_json.as_bytes())?;
    
    if let Some(semantics) = &frame.semantics {
        println!("Detected semantic type: {:?}", semantics.semantic_type);
        match &semantics.semantic_type {
            SemanticType::Geospatial { coordinate_system, geometry_type } => {
                println!("  Coordinate system: {}", coordinate_system);
                println!("  Geometry type: {}", geometry_type);
            }
            _ => println!("  Not detected as geospatial"),
        }
    }

    // Example 6: Custom parser configuration
    println!("\n=== Example 6: Custom Configuration ===");
    let custom_config = ParseConfig {
        detect_semantics: true,
        max_size_mb: 10,
        stream_large_arrays: true,
        stream_threshold: 100,
    };
    
    let custom_parser = Parser::with_config(custom_config);
    let frame = custom_parser.parse(json_obj.as_bytes())?;
    println!("Parsed with custom config: {} bytes", frame.payload.len());

    // Show parser statistics
    println!("\n=== Parser Statistics ===");
    let stats = parser.stats();
    println!("Total parses: {}", stats.total_parses);
    println!("Semantic detections: {}", stats.semantic_detections);
    println!("Average parse time: {:.2}ms", stats.avg_parse_time_ms);

    Ok(())
}