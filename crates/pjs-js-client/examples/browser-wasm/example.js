/**
 * PJS WASM Usage Example (Node.js / Bundler)
 *
 * This example shows how to use pjs-wasm in a Node.js environment
 * or with a bundler like Webpack, Vite, or Rollup.
 */

// Import WASM module (ensure pkg/ is built first)
import init, { PjsParser, PriorityConstants, PriorityConfigBuilder } from './pkg/pjs_wasm.js';
import { readFile } from 'fs/promises';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

async function main() {
    // Initialize WASM module
    console.log('Initializing WASM...');

    // Node.js requires loading WASM from file system
    const __filename = fileURLToPath(import.meta.url);
    const __dirname = dirname(__filename);
    const wasmPath = join(__dirname, 'pkg', 'pjs_wasm_bg.wasm');
    const wasmBuffer = await readFile(wasmPath);

    await init(wasmBuffer);
    console.log('WASM initialized successfully!\n');

    // Example 1: Basic frame generation
    console.log('=== Example 1: Basic Usage ===');
    const parser = new PjsParser();
    console.log(`Parser version: ${PjsParser.version()}`);

    const sampleData = {
        id: 1,
        name: "Alice Johnson",
        email: "alice@example.com",
        bio: "Software developer passionate about WebAssembly",
        analytics: {
            views: 1523,
            clicks: 287
        }
    };

    const frames = parser.generateFrames(
        JSON.stringify(sampleData),
        PriorityConstants.MEDIUM  // Threshold: 50
    );

    console.log(`Generated ${frames.length} frames:`);
    frames.forEach((frame, i) => {
        console.log(`  Frame ${i}: ${frame.frame_type} (Priority: ${frame.priority})`);
    });
    console.log('');

    // Example 2: Custom configuration
    console.log('=== Example 2: Custom Configuration ===');
    const config = new PriorityConfigBuilder()
        .addCriticalField('product_id')
        .addHighField('product_name')
        .addLowPattern('debug')
        .addBackgroundPattern('recommendations');

    const customParser = PjsParser.withConfig(config);

    const productData = {
        product_id: "P123",
        product_name: "WASM Widget",
        description: "High-performance widget",
        debug_info: {
            version: "1.0.0"
        },
        recommendations: [1, 2, 3, 4, 5]
    };

    const customFrames = customParser.generateFrames(
        JSON.stringify(productData),
        PriorityConstants.LOW
    );

    console.log(`Generated ${customFrames.length} frames with custom config:`);
    customFrames.forEach((frame, i) => {
        console.log(`  Frame ${i}: ${frame.frame_type} (Priority: ${frame.priority})`);
    });
    console.log('');

    // Example 3: Priority filtering
    console.log('=== Example 3: Priority Filtering ===');
    const largeData = {
        id: 1,
        name: "Critical Data",
        description: "Medium priority field",
        metadata: "Low priority field",
        logs: ["background", "priority", "data"]
    };

    // Generate with HIGH threshold - only CRITICAL and HIGH fields
    const highPriorityFrames = parser.generateFrames(
        JSON.stringify(largeData),
        PriorityConstants.HIGH  // 80
    );

    console.log(`HIGH threshold (80): ${highPriorityFrames.length} frames`);

    // Generate with BACKGROUND threshold - all fields
    const allFrames = parser.generateFrames(
        JSON.stringify(largeData),
        PriorityConstants.BACKGROUND  // 10
    );

    console.log(`BACKGROUND threshold (10): ${allFrames.length} frames`);
    console.log('');

    // Example 4: Performance measurement
    console.log('=== Example 4: Performance ===');
    const largeJson = {
        users: Array.from({ length: 100 }, (_, i) => ({
            id: i,
            name: `User ${i}`,
            email: `user${i}@example.com`,
            metadata: {
                created: new Date().toISOString(),
                updated: new Date().toISOString()
            }
        }))
    };

    const startTime = performance.now();
    const perfFrames = parser.generateFrames(
        JSON.stringify(largeJson),
        PriorityConstants.MEDIUM
    );
    const endTime = performance.now();

    console.log(`Generated ${perfFrames.length} frames in ${(endTime - startTime).toFixed(2)}ms`);
    console.log(`Average: ${((endTime - startTime) / perfFrames.length).toFixed(3)}ms per frame`);
    console.log('');

    // Example 5: Error handling
    console.log('=== Example 5: Error Handling ===');
    try {
        // Invalid JSON
        parser.generateFrames('{ invalid json }', 50);
    } catch (error) {
        console.log(`✓ Caught parse error: ${error.message}`);
    }

    try {
        // Invalid priority (0 is not allowed)
        parser.generateFrames('{"valid": "json"}', 0);
    } catch (error) {
        console.log(`✓ Caught priority error: ${error.message}`);
    }

    console.log('\n✅ All examples completed successfully!');
}

// Run examples
main().catch(console.error);
