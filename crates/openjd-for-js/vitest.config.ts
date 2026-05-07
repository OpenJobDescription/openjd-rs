import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["js-tests/**/*.test.{js,ts}"],
    testTimeout: 30000,
  },
});
