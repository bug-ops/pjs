/** @type {import('jest').Config} */
export default {
  preset: 'ts-jest/presets/default-esm',
  testEnvironment: 'node',

  testMatch: ['<rootDir>/tests/cross-stack/**/*.test.ts'],

  extensionsToTreatAsEsm: ['.ts'],
  moduleNameMapper: {
    '^(\\.{1,2}/.*)\\.js$': '$1'
  },

  transform: {
    '^.+\\.tsx?$': [
      'ts-jest',
      {
        useESM: true,
        tsconfig: {
          module: 'ESNext',
          rootDir: '.',
          target: 'ES2020'
        }
      }
    ]
  },

  // tests/setup.ts is deliberately not loaded: it replaces WebSocket and fetch with mocks.
  testTimeout: 60000,
  verbose: true
};
