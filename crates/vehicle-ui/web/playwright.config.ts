import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: "http://127.0.0.1:8080",
    browserName: "chromium",
  },
  webServer: {
    command:
      "cargo run --quiet --manifest-path ../../../Cargo.toml -p vehicle-ui --bin celerity-ui",
    url: "http://127.0.0.1:8080",
    reuseExistingServer: false,
  },
});
