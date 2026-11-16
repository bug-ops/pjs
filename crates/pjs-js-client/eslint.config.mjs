import js from '@eslint/js';
import tsPlugin from '@typescript-eslint/eslint-plugin';
import tsParser from '@typescript-eslint/parser';

export default [
  { ignores: ['dist/**', 'node_modules/**', 'coverage/**', '**/*.js', '**/*.mjs'] },
  js.configs.recommended,
  ...tsPlugin.configs['flat/recommended-type-checked'],
  {
    files: ['src/**/*.ts'],
    languageOptions: {
      parser: tsParser,
      parserOptions: { project: './tsconfig.json', tsconfigRootDir: import.meta.dirname }
    },
    rules: {
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      '@typescript-eslint/no-explicit-any': 'warn',
      '@typescript-eslint/no-unsafe-assignment': 'warn',
      '@typescript-eslint/no-unsafe-member-access': 'warn',
      '@typescript-eslint/no-unsafe-call': 'warn',
      '@typescript-eslint/no-unsafe-return': 'warn',
      '@typescript-eslint/no-unsafe-argument': 'warn',
      '@typescript-eslint/restrict-template-expressions': 'warn',
      'prefer-const': 'error',
      'no-var': 'error',
      'object-shorthand': 'error',
      'prefer-template': 'error',
      'sort-imports': ['error', { ignoreCase: true, ignoreDeclarationSort: true, ignoreMemberSort: false }]
    }
  },
  {
    // Transports implement the async `Transport` contract with synchronous bodies.
    files: ['src/transport/**/*.ts'],
    rules: { '@typescript-eslint/require-await': 'off' }
  }
];
