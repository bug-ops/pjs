import init, { PriorityStream, version } from '../pkg/pjs_wasm.js';

// Sample data presets
const PRESETS = {
    small: {
        name: 'User Profile',
        size: '~1KB',
        data: {
            id: 12345,
            status: 'active',
            name: 'Alice Johnson',
            email: 'alice@example.com',
            avatar: 'https://example.com/avatars/alice.jpg',
            bio: 'Software developer passionate about performance and user experience.',
            location: {
                city: 'San Francisco',
                country: 'USA',
                timezone: 'PST'
            },
            preferences: {
                theme: 'dark',
                notifications: true
            }
        }
    },
    medium: {
        name: 'Product Catalog',
        size: '~10KB',
        data: {
            metadata: {
                total: 50,
                category: 'electronics',
                lastUpdated: '2024-01-15T10:30:00Z'
            },
            products: Array.from({ length: 50 }, (_, i) => ({
                id: 1000 + i,
                name: `Product ${i + 1}`,
                price: Math.floor(Math.random() * 1000) + 50,
                rating: (Math.random() * 5).toFixed(1),
                stock: Math.floor(Math.random() * 100),
                category: ['laptop', 'phone', 'tablet', 'accessory'][i % 4],
                description: `High-quality ${['laptop', 'phone', 'tablet', 'accessory'][i % 4]} with advanced features.`,
                specs: {
                    weight: `${(Math.random() * 2 + 0.5).toFixed(1)}kg`,
                    dimensions: `${Math.floor(Math.random() * 20 + 10)}x${Math.floor(Math.random() * 20 + 10)}x${Math.floor(Math.random() * 5 + 1)}cm`,
                    warranty: '2 years'
                }
            }))
        }
    },
    large: {
        name: 'Analytics Data',
        size: '~100KB',
        data: {
            dashboard: {
                period: '2024-Q1',
                generated: '2024-01-15T12:00:00Z',
                summary: {
                    totalUsers: 125430,
                    activeUsers: 98234,
                    revenue: 5432100.50,
                    transactions: 234567
                }
            },
            dailyMetrics: Array.from({ length: 90 }, (_, i) => ({
                date: `2024-${String(Math.floor(i / 30) + 1).padStart(2, '0')}-${String((i % 30) + 1).padStart(2, '0')}`,
                users: Math.floor(Math.random() * 5000 + 10000),
                sessions: Math.floor(Math.random() * 8000 + 15000),
                pageviews: Math.floor(Math.random() * 50000 + 100000),
                revenue: (Math.random() * 50000 + 10000).toFixed(2),
                conversions: Math.floor(Math.random() * 500 + 100),
                bounceRate: (Math.random() * 0.3 + 0.2).toFixed(3),
                avgSessionDuration: Math.floor(Math.random() * 300 + 120),
                topPages: [
                    { path: '/home', views: Math.floor(Math.random() * 5000) },
                    { path: '/products', views: Math.floor(Math.random() * 3000) },
                    { path: '/about', views: Math.floor(Math.random() * 1000) }
                ],
                devices: {
                    mobile: Math.floor(Math.random() * 3000),
                    desktop: Math.floor(Math.random() * 2000),
                    tablet: Math.floor(Math.random() * 500)
                }
            })),
            userSegments: Array.from({ length: 20 }, (_, i) => ({
                segment: `Segment ${i + 1}`,
                users: Math.floor(Math.random() * 10000 + 1000),
                revenue: (Math.random() * 100000).toFixed(2),
                avgOrderValue: (Math.random() * 200 + 50).toFixed(2),
                lifetimeValue: (Math.random() * 1000 + 100).toFixed(2)
            }))
        }
    }
};

class PJSDemo {
    constructor() {
        this.stream = null;
        this.currentTransport = 'wasm';
        this.metrics = {
            startTime: 0,
            firstFrameTime: 0,
            memoryBefore: 0,
            totalFrames: 0,
            currentFrame: 0
        };
    }

    async init() {
        try {
            await init();
            document.getElementById('version').textContent = `PJS WASM v${version()}`;
            this.setupEventListeners();
            this.loadPreset('medium');
        } catch (error) {
            console.error('Failed to initialize WASM:', error);
            document.getElementById('version').textContent = `Error: ${error.message}`;
        }
    }

    setupEventListeners() {
        // Main controls
        document.getElementById('streamBtn').addEventListener('click', () => this.startStream());
        document.getElementById('clearBtn').addEventListener('click', () => this.clearOutput());

        // Transport switcher
        document.querySelectorAll('input[name="transport"]').forEach(radio => {
            radio.addEventListener('change', (e) => {
                this.currentTransport = e.target.value;
                this.updateTransportIndicator();
            });
        });

        // Preset selector
        document.getElementById('presetSelect').addEventListener('change', (e) => {
            if (e.target.value !== 'custom') {
                this.loadPreset(e.target.value);
            }
        });

        // Input size tracking
        const jsonInput = document.getElementById('jsonInput');
        jsonInput.addEventListener('input', () => this.updateInputSize());

        // Keyboard shortcuts
        document.addEventListener('keydown', (e) => {
            if (e.key === 'Enter' && e.ctrlKey) {
                e.preventDefault();
                this.startStream();
            } else if (e.key === 'Escape') {
                e.preventDefault();
                this.clearOutput();
            }
        });

        // Benchmark button
        document.getElementById('benchmarkBtn').addEventListener('click', () => this.runBenchmark());

        // Initial update
        this.updateInputSize();
    }

    loadPreset(presetName) {
        const preset = PRESETS[presetName];
        if (preset) {
            const jsonInput = document.getElementById('jsonInput');
            jsonInput.value = JSON.stringify(preset.data, null, 2);
            this.updateInputSize();
            document.getElementById('presetSelect').value = presetName;
        }
    }

    updateInputSize() {
        const jsonInput = document.getElementById('jsonInput');
        const bytes = new Blob([jsonInput.value]).size;
        document.getElementById('inputSize').textContent = this.formatBytes(bytes);
    }

    updateTransportIndicator() {
        const status = document.getElementById('streamingStatus');
        status.textContent = this.currentTransport === 'wasm' ? '⚡ WASM' : '🌐 HTTP Mock';
    }

    async startStream() {
        const jsonInput = document.getElementById('jsonInput').value;
        const minPriority = parseInt(document.getElementById('prioritySelect').value);
        const output = document.getElementById('output');

        // Clear previous output
        output.innerHTML = '<div class="frame-list" id="frameList"></div>';
        const frameList = document.getElementById('frameList');

        // Show loading state
        const status = document.getElementById('streamingStatus');
        status.textContent = 'Streaming...';
        status.classList.add('active');

        // Reset metrics
        this.resetMetrics();

        try {
            if (this.currentTransport === 'wasm') {
                await this.streamWithWASM(jsonInput, minPriority, frameList);
            } else {
                await this.streamWithHTTPMock(jsonInput, minPriority, frameList);
            }
        } catch (error) {
            output.innerHTML = `<div class="error">Error: ${this.escapeHtml(error.message)}</div>`;
        } finally {
            status.classList.remove('active');
            status.textContent = '';
        }
    }

    resetMetrics() {
        this.metrics = {
            startTime: performance.now(),
            firstFrameTime: 0,
            memoryBefore: this.getMemoryUsage(),
            totalFrames: 0,
            currentFrame: 0
        };
        this.updateRealtimeMetrics();
    }

    async streamWithWASM(jsonInput, minPriority, frameList) {
        this.stream = new PriorityStream();
        this.stream.setMinPriority(minPriority);

        this.stream.onFrame((frame) => {
            this.handleFrame(frame, frameList);
        });

        this.stream.onComplete((stats) => {
            this.handleComplete(stats);
        });

        this.stream.onError((error) => {
            this.handleError(error, frameList);
        });

        this.stream.start(jsonInput);
    }

    async streamWithHTTPMock(jsonInput, minPriority, frameList) {
        // Simulate HTTP delay
        const networkDelay = 50; // ms per frame

        // Parse JSON traditionally
        const startParse = performance.now();
        const data = JSON.parse(jsonInput);
        const parseTime = performance.now() - startParse;

        // Simulate receiving the entire response
        await this.delay(networkDelay);

        // Create a mock stream to show the difference
        const frames = this.generateMockFrames(data);
        this.metrics.totalFrames = frames.length;

        for (const frame of frames) {
            await this.delay(networkDelay);
            this.handleFrame(frame, frameList);
        }

        this.handleComplete({
            totalFrames: frames.length,
            patchFrames: frames.length - 2,
            bytesProcessed: new Blob([jsonInput]).size,
            durationMs: performance.now() - this.metrics.startTime
        });
    }

    generateMockFrames(data) {
        // Simplified mock frame generation
        return [
            {
                type: 'skeleton',
                sequence: 0,
                priority: 255,
                payload: JSON.stringify({})
            },
            {
                type: 'patch',
                sequence: 1,
                priority: 100,
                payload: JSON.stringify(data)
            },
            {
                type: 'complete',
                sequence: 2,
                priority: 255,
                payload: JSON.stringify({ status: 'complete' })
            }
        ];
    }

    handleFrame(frame, frameList) {
        if (this.metrics.firstFrameTime === 0) {
            this.metrics.firstFrameTime = performance.now() - this.metrics.startTime;
        }

        this.metrics.currentFrame++;

        const frameEl = document.createElement('div');
        const priorityClass = frame.type === 'skeleton' ? 'skeleton' :
                            frame.type === 'complete' ? 'complete' :
                            this.getPriorityClass(frame.priority);

        frameEl.className = `frame ${priorityClass}`;

        let payload = frame.payload;
        try {
            const obj = JSON.parse(payload);
            payload = JSON.stringify(obj, null, 2);
            if (payload.length > 200) {
                payload = payload.substring(0, 200) + '...';
            }
        } catch {}

        frameEl.innerHTML = `
            <div class="frame-header">
                <span class="frame-type">${frame.type} #${frame.sequence}</span>
                <span class="frame-priority">Priority: ${frame.priority}</span>
            </div>
            <div class="frame-payload">${this.escapeHtml(payload)}</div>
        `;

        frameList.appendChild(frameEl);
        frameList.scrollTop = frameList.scrollHeight;

        this.updateRealtimeMetrics();
    }

    handleComplete(stats) {
        this.updateStats(stats);
        this.updateRealtimeMetrics(true);
    }

    handleError(error, frameList) {
        const errorEl = document.createElement('div');
        errorEl.className = 'error';
        errorEl.textContent = `Error: ${error}`;
        frameList.appendChild(errorEl);
    }

    updateStats(stats) {
        if (stats) {
            document.getElementById('statFrames').textContent = stats.totalFrames;
            document.getElementById('statPatches').textContent = stats.patchFrames;
            document.getElementById('statBytes').textContent = this.formatBytes(stats.bytesProcessed);
            document.getElementById('statTime').textContent = stats.durationMs.toFixed(2);
        } else {
            document.getElementById('statFrames').textContent = '-';
            document.getElementById('statPatches').textContent = '-';
            document.getElementById('statBytes').textContent = '-';
            document.getElementById('statTime').textContent = '-';
        }
    }

    updateRealtimeMetrics(complete = false) {
        // Memory usage
        const memoryMB = this.getMemoryUsage();
        document.getElementById('metricMemory').textContent = memoryMB ? `${memoryMB.toFixed(1)} MB` : 'N/A';

        // Throughput
        const elapsed = performance.now() - this.metrics.startTime;
        if (elapsed > 0 && this.metrics.currentFrame > 0) {
            const framesPerSec = (this.metrics.currentFrame / elapsed * 1000).toFixed(0);
            document.getElementById('metricThroughput').textContent = `${framesPerSec} fps`;
        }

        // Time to first frame
        if (this.metrics.firstFrameTime > 0) {
            document.getElementById('metricTTFF').textContent = `${this.metrics.firstFrameTime.toFixed(2)} ms`;
        }

        // Progress
        const progress = this.metrics.totalFrames > 0
            ? Math.round((this.metrics.currentFrame / this.metrics.totalFrames) * 100)
            : 0;
        document.getElementById('metricProgress').textContent = `${progress}%`;
        document.getElementById('progressBar').style.width = `${progress}%`;
    }

    getMemoryUsage() {
        if (performance.memory) {
            return performance.memory.usedJSHeapSize / 1024 / 1024;
        }
        return null;
    }

    async runBenchmark() {
        const btn = document.getElementById('benchmarkBtn');
        const resultsDiv = document.getElementById('comparisonResults');

        btn.disabled = true;
        btn.textContent = 'Running Benchmark...';

        resultsDiv.innerHTML = '<div class="loading"><div class="spinner"></div><span>Running benchmark...</span></div>';

        try {
            const testData = PRESETS.medium.data;
            const jsonString = JSON.stringify(testData);

            // Benchmark WASM
            const wasmResults = await this.benchmarkWASM(jsonString);

            // Benchmark traditional JSON parsing
            const traditionalResults = await this.benchmarkTraditional(jsonString);

            // Display results
            this.displayBenchmarkResults(wasmResults, traditionalResults);
        } catch (error) {
            resultsDiv.innerHTML = `<div class="error">Benchmark failed: ${this.escapeHtml(error.message)}</div>`;
        } finally {
            btn.disabled = false;
            btn.textContent = '▶ Run Benchmark';
        }
    }

    async benchmarkWASM(jsonString) {
        const iterations = 100;
        const times = [];

        for (let i = 0; i < iterations; i++) {
            const stream = new PriorityStream();

            const start = performance.now();
            await new Promise((resolve) => {
                stream.onComplete(() => resolve());
                stream.start(jsonString);
            });
            times.push(performance.now() - start);
        }

        return {
            avgTime: times.reduce((a, b) => a + b, 0) / times.length,
            minTime: Math.min(...times),
            maxTime: Math.max(...times)
        };
    }

    async benchmarkTraditional(jsonString) {
        const iterations = 100;
        const times = [];

        for (let i = 0; i < iterations; i++) {
            const start = performance.now();
            JSON.parse(jsonString);
            times.push(performance.now() - start);
        }

        return {
            avgTime: times.reduce((a, b) => a + b, 0) / times.length,
            minTime: Math.min(...times),
            maxTime: Math.max(...times)
        };
    }

    displayBenchmarkResults(wasmResults, traditionalResults) {
        const speedup = (traditionalResults.avgTime / wasmResults.avgTime).toFixed(2);
        const percentFaster = ((1 - wasmResults.avgTime / traditionalResults.avgTime) * 100).toFixed(1);

        document.getElementById('comparisonResults').innerHTML = `
            <div class="comparison-grid">
                <div class="comparison-card">
                    <div class="comparison-title">PJS WASM</div>
                    <div class="comparison-metrics">
                        <div class="comparison-metric">
                            <span class="comparison-metric-label">Avg:</span>
                            <span class="comparison-metric-value">${wasmResults.avgTime.toFixed(2)} ms</span>
                        </div>
                        <div class="comparison-metric">
                            <span class="comparison-metric-label">Min:</span>
                            <span class="comparison-metric-value">${wasmResults.minTime.toFixed(2)} ms</span>
                        </div>
                        <div class="comparison-metric">
                            <span class="comparison-metric-label">Max:</span>
                            <span class="comparison-metric-value">${wasmResults.maxTime.toFixed(2)} ms</span>
                        </div>
                    </div>
                </div>
                <div class="comparison-card">
                    <div class="comparison-title">Traditional JSON.parse</div>
                    <div class="comparison-metrics">
                        <div class="comparison-metric">
                            <span class="comparison-metric-label">Avg:</span>
                            <span class="comparison-metric-value">${traditionalResults.avgTime.toFixed(2)} ms</span>
                        </div>
                        <div class="comparison-metric">
                            <span class="comparison-metric-label">Min:</span>
                            <span class="comparison-metric-value">${traditionalResults.minTime.toFixed(2)} ms</span>
                        </div>
                        <div class="comparison-metric">
                            <span class="comparison-metric-label">Max:</span>
                            <span class="comparison-metric-value">${traditionalResults.maxTime.toFixed(2)} ms</span>
                        </div>
                    </div>
                </div>
                <div class="comparison-card" style="grid-column: span 2;">
                    <div class="comparison-speedup">
                        <div class="speedup-value">${speedup}x</div>
                        <div class="speedup-label">Speedup (${percentFaster}% faster)</div>
                    </div>
                </div>
            </div>
        `;
    }

    clearOutput() {
        document.getElementById('output').innerHTML = `
            <div class="empty-state">
                Click "Stream Frames" to begin streaming
            </div>
        `;
        this.updateStats(null);
        this.metrics = {
            startTime: 0,
            firstFrameTime: 0,
            memoryBefore: 0,
            totalFrames: 0,
            currentFrame: 0
        };

        // Reset real-time metrics
        document.getElementById('metricMemory').textContent = '-';
        document.getElementById('metricThroughput').textContent = '-';
        document.getElementById('metricTTFF').textContent = '-';
        document.getElementById('metricProgress').textContent = '-';
        document.getElementById('progressBar').style.width = '0%';
    }

    getPriorityClass(priority) {
        if (priority >= 100) return 'critical';
        if (priority >= 80) return 'high';
        if (priority >= 50) return 'medium';
        if (priority >= 25) return 'low';
        return 'background';
    }

    formatBytes(bytes) {
        if (bytes < 1024) return bytes + ' B';
        if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
        return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
    }

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }

    delay(ms) {
        return new Promise(resolve => setTimeout(resolve, ms));
    }
}

// Initialize the demo
const demo = new PJSDemo();
demo.init();
