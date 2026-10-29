import { defineConfig } from "playwright/test";

export default defineConfig({
  testDir: "./tests/ui",
  timeout: 15_000,
  expect: { timeout: 3_000 },
  fullyParallel: true,
  workers: 2,
  retries: 0,
  forbidOnly: !!process.env.CI,
  outputDir: "test-results/ui",
  reporter: [
    ["list"],
    ["html", { outputFolder: "playwright-report", open: "never" }],
    ["json", { outputFile: "test-results/ui-results.json" }],
  ],
  use: {
    baseURL: "http://127.0.0.1:1430",
    browserName: "chromium",
    viewport: { width: 1280, height: 800 },
    locale: "en-US",
    actionTimeout: 3_000,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: {
    command: "pnpm exec vite --host 127.0.0.1 --port 1430 --strictPort",
    url: "http://127.0.0.1:1430",
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
