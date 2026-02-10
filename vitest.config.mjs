import { defineConfig } from "vitest/config"

export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    exclude: ["tmp/**", "dist/**", "node_modules/**"],
    globals: true,
    environment: "node",
    env: {
      VDE_TEST_MODE: "true",
    },
    coverage: {
      provider: "v8",
      reporter: ["text", "json", "html"],
      exclude: ["node_modules/", "dist/", "**/*.d.ts", "**/*.config.*", "**/__tests__/**"],
    },
    testTimeout: 10000,
    hookTimeout: 10000,
  },
})
