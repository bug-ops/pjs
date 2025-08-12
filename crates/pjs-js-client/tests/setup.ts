/**
 * Jest test setup file
 * 
 * Global test configuration and mocks for PJS client tests.
 */

// Mock WebSocket for Node.js environment
global.WebSocket = class MockWebSocket extends EventTarget {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;

  readyState = MockWebSocket.CONNECTING;
  url: string;
  protocol: string;

  constructor(url: string, protocol?: string) {
    super();
    this.url = url;
    this.protocol = protocol || '';
    
    // Simulate connection opening
    setTimeout(() => {
      this.readyState = MockWebSocket.OPEN;
      this.dispatchEvent(new Event('open'));
    }, 10);
  }

  send(data: string | ArrayBufferLike | Blob | ArrayBufferView) {
    if (this.readyState !== MockWebSocket.OPEN) {
      throw new Error('WebSocket is not open');
    }
    // Mock implementation - could emit message events for testing
  }

  close(code?: number, reason?: string) {
    this.readyState = MockWebSocket.CLOSING;
    setTimeout(() => {
      this.readyState = MockWebSocket.CLOSED;
      const closeEvent = new CloseEvent('close', { code, reason });
      this.dispatchEvent(closeEvent);
    }, 10);
  }
} as any;

// Mock EventSource for Node.js environment
global.EventSource = class MockEventSource extends EventTarget {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSED = 2;

  readyState = MockEventSource.CONNECTING;
  url: string;
  withCredentials: boolean;

  constructor(url: string, eventSourceInitDict?: EventSourceInit) {
    super();
    this.url = url;
    this.withCredentials = eventSourceInitDict?.withCredentials || false;
    
    // Simulate connection opening
    setTimeout(() => {
      this.readyState = MockEventSource.OPEN;
      this.dispatchEvent(new Event('open'));
    }, 10);
  }

  close() {
    this.readyState = MockEventSource.CLOSED;
    this.dispatchEvent(new Event('close'));
  }
} as any;

// Mock fetch with basic functionality
global.fetch = jest.fn().mockImplementation((url: string, options?: RequestInit) => {
  const mockResponse = {
    ok: true,
    status: 200,
    statusText: 'OK',
    headers: new Headers(),
    json: async () => ({ session_id: 'test-session' }),
    text: async () => '{"type":"skeleton","priority":100,"data":{"test":true}}',
    body: {
      getReader: () => ({
        read: async () => ({ done: true, value: undefined }),
        releaseLock: () => {}
      })
    }
  };
  
  return Promise.resolve(mockResponse as Response);
});

// Console suppression for cleaner test output
const originalConsole = global.console;
global.console = {
  ...originalConsole,
  log: jest.fn(),
  warn: jest.fn(),
  error: jest.fn(),
  debug: jest.fn()
};

// Test utilities
global.createMockFrame = (type: string, priority: number, data?: any) => ({
  type,
  priority,
  timestamp: Date.now(),
  ...(data && { data }),
  ...(type === 'patch' && { patches: data?.patches || [] })
});

global.createMockClient = (config: any = {}) => {
  const { PJSClient } = require('../src/core/client');
  return new PJSClient({
    baseUrl: 'http://localhost:3000',
    debug: false,
    ...config
  });
};

// Cleanup after tests
afterEach(() => {
  jest.clearAllMocks();
});