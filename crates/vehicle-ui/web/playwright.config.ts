import { defineConfig } from "@playwright/test";

const port = process.env.VEHICLE_UI_PORT ?? "8080";
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL,
    browserName: "chromium",
  },
  webServer: {
    command:
      "cargo run --quiet --manifest-path ../../../Cargo.toml -p vehicle-ui --bin celerity-ui",
    url: baseURL,
    env: { VEHICLE_UI_PORT: port },
    reuseExistingServer: false,
  },
});
