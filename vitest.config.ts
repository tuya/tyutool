import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const pkg = JSON.parse(readFileSync(path.join(__dirname, 'package.json'), 'utf-8')) as { version: string };

export default defineConfig({
  plugins: [vue()],
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts', 'vite/**/*.test.ts', 'scripts/**/*.test.ts'],
    passWithNoTests: false,
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      exclude: ['src/**/*.test.ts', 'src/**/*.d.ts', 'src/main.ts', 'src/vite-env.d.ts'],
      // Vitest 4's V8 remapping is more accurate than Vitest 3's, so the
      // percentages are lower even though the covered code is unchanged.
      // Keep the thresholds just below the new baseline so obvious regressions
      // fail CI without flaking on noise.
      thresholds: {
        statements: 78,
        branches: 73,
        functions: 80,
        lines: 80,
      },
    },
  },
});
