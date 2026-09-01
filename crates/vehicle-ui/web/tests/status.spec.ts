import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

const here = path.dirname(fileURLToPath(import.meta.url));
const fixtures = path.resolve(here, "../fixtures/status");
const current = readFixture("current.json");
const producerStale = readFixture("producer-stale.json");
const stale = readFixture("stale.json");
const unavailable = readFixture("unavailable.json");
const unknown = readFixture("unknown.json");
const evidence = path.resolve(
  here,
  "../../../../.agent/work/local-status-viewer/evidence/ui",
);
fs.mkdirSync(evidence, { recursive: true });

function readFixture(name: string): object {
  return JSON.parse(
    fs.readFileSync(path.join(fixtures, name), "utf8"),
  ) as object;
}

const viewports = [
  { name: "wide", width: 1440, height: 900 },
  { name: "narrow", width: 700, height: 900 },
] as const;

const scenarios = [
  {
    name: "current",
    fixture: current,
    banner: /CONNECTED \/ RUNTIME UPDATING/,
  },
  {
    name: "producer-stale",
    fixture: producerStale,
    banner: /CONNECTED \/ RUNTIME UPDATE DELAYED/,
  },
  {
    name: "stale",
    fixture: stale,
    banner: /DIAGNOSTICS STALE/,
  },
  {
    name: "unavailable",
    fixture: unavailable,
    banner: /STATUS UNAVAILABLE/,
  },
  {
    name: "unknown",
    fixture: unknown,
    banner: /CONNECTED \/ RUNTIME UPDATING/,
  },
] as const;

for (const scenario of scenarios) {
  for (const viewport of viewports) {
    test(`${scenario.name} ${viewport.name} is complete and accessible`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      const consoleErrors: string[] = [];
      const failedRequests: string[] = [];
      page.on("console", (message) => {
        if (message.type() === "error") consoleErrors.push(message.text());
      });
      page.on("requestfailed", (request) => failedRequests.push(request.url()));
      await page.route("**/v1/status", (route) =>
        route.fulfill({
          status: 200,
          contentType: "application/json",
          json: scenario.fixture,
        }),
      );

      await page.goto("/");
      const banner = page.getByRole("status");
      await expect(banner).toHaveAccessibleName(scenario.banner);
      await expect(page.locator(".status-card")).toHaveCount(9);
      await expect(
        page.getByRole("heading", { name: "Run storage" }),
      ).toBeVisible();
      if (scenario.name === "producer-stale") {
        const temperatureDetail = page
          .locator(".status-card")
          .filter({
            has: page.getByRole("heading", { name: "Coolant temperature" }),
          })
          .locator(".status-card__detail");
        const ageText = await temperatureDetail.textContent();
        expect(displayedAgeMs(ageText ?? "")).toBeGreaterThanOrEqual(600);
      }
      await assertViewportContainsAllCardText(
        page,
        viewport.width,
        viewport.height,
      );
      await assertContrastTargets(page);
      const axe = await new AxeBuilder({ page }).analyze();
      expect(
        axe.violations,
        "axe automated rules; not screen-reader evidence",
      ).toEqual([]);
      await page.keyboard.press("Tab");
      await expect(page.locator(".skip-link")).toBeFocused();
      await page
        .locator(".skip-link")
        .evaluate((element) => (element as HTMLElement).blur());
      expect(consoleErrors).toEqual([]);
      expect(failedRequests).toEqual([]);
      await page.screenshot({
        path: path.join(evidence, `${scenario.name}-${viewport.name}.png`),
        fullPage: false,
      });
    });
  }
}

for (const viewport of viewports) {
  test(`observer unreachable ${viewport.name} replaces values`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    const consoleErrors: string[] = [];
    let requests = 0;
    page.on("console", (message) => {
      if (message.type() === "error") consoleErrors.push(message.text());
    });
    await page.route("**/v1/status", async (route) => {
      requests += 1;
      if (requests === 1) {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          json: current,
        });
      } else {
        await route.abort("connectionrefused");
      }
    });

    await page.goto("/");
    const banner = page.getByRole("status");
    await expect(banner).toHaveAccessibleName(/CONNECTED \/ RUNTIME UPDATING/);
    await expect(banner).toHaveAccessibleName("STATUS UNAVAILABLE", {
      timeout: 3_000,
    });
    await expect(page.locator(".banner p")).toContainText(
      "LOCAL OBSERVER UNREACHABLE",
    );
    await expect(page.locator(".status-card__value").first()).toHaveText(
      "UNAVAILABLE",
    );
    await assertViewportContainsAllCardText(
      page,
      viewport.width,
      viewport.height,
    );
    await assertContrastTargets(page);
    const axe = await new AxeBuilder({ page }).analyze();
    expect(
      axe.violations,
      "axe automated rules; not screen-reader evidence",
    ).toEqual([]);
    expect(consoleErrors.length).toBeGreaterThan(0);
    expect(
      consoleErrors.every((message) =>
        message.includes("net::ERR_CONNECTION_REFUSED"),
      ),
      `only the deliberate observer failure is allowed: ${consoleErrors.join(" | ")}`,
    ).toBe(true);
    await page.screenshot({
      path: path.join(evidence, `observer-unreachable-${viewport.name}.png`),
      fullPage: false,
    });
  });
}

test("production response renders the shipped asset with strict headers", async ({
  page,
}) => {
  await page.route("**/v1/status", (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      json: current,
    }),
  );
  const response = await page.goto("/");
  expect(response?.status()).toBe(200);
  expect(response?.headers()["content-security-policy"]).toBe(
    "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; img-src data:",
  );
  expect(response?.headers()["x-content-type-options"]).toBe("nosniff");
  await expect(
    page.getByRole("heading", { name: "Runtime status" }),
  ).toBeVisible();
  await expect(page.getByRole("status")).toHaveAccessibleName(
    "CONNECTED / RUNTIME UPDATING",
  );
});

test("healthy polling keeps the live announcement stable", async ({ page }) => {
  let requests = 0;
  await page.route("**/v1/status", (route) => {
    requests += 1;
    route.fulfill({
      status: 200,
      contentType: "application/json",
      json: {
        ...current,
        last_success_age_ms: 125 + requests * 1_000,
      },
    });
  });

  await page.goto("/");
  const banner = page.getByRole("status");
  await expect(banner).toHaveAccessibleName("CONNECTED / RUNTIME UPDATING");
  const announcedName = await banner.getAttribute("aria-label");
  await expect
    .poll(() => requests, { timeout: 3_000 })
    .toBeGreaterThanOrEqual(2);
  await expect(banner).toHaveAccessibleName("CONNECTED / RUNTIME UPDATING");
  expect(await banner.getAttribute("aria-label")).toBe(announcedName);
});

function displayedAgeMs(value: string): number {
  const match = /^AGE NOW ([\d.]+) (MS|S)$/.exec(value);
  expect(match, `rendered age: ${value}`).not.toBeNull();
  return Number(match![1]) * (match![2] === "S" ? 1_000 : 1);
}

async function assertViewportContainsAllCardText(
  page: Page,
  width: number,
  height: number,
) {
  const dimensions = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    clientWidth: document.documentElement.clientWidth,
    scrollHeight: document.documentElement.scrollHeight,
    clientHeight: document.documentElement.clientHeight,
  }));
  expect(dimensions.scrollWidth).toBeLessThanOrEqual(width);
  expect(dimensions.clientWidth).toBe(width);
  expect(dimensions.scrollHeight).toBeLessThanOrEqual(height);
  expect(dimensions.clientHeight).toBe(height);
  for (const value of await page
    .locator(".status-card__value, .status-card__detail")
    .all()) {
    await expect(value).toBeVisible();
    const fits = await value.evaluate((element) => {
      const style = getComputedStyle(element);
      const bounds = element.getBoundingClientRect();
      return (
        style.overflow !== "hidden" &&
        style.textOverflow !== "ellipsis" &&
        bounds.left >= 0 &&
        bounds.right <= window.innerWidth &&
        bounds.top >= 0 &&
        bounds.bottom <= window.innerHeight
      );
    });
    expect(fits, `fully visible value: ${await value.textContent()}`).toBe(
      true,
    );
  }
}

async function assertContrastTargets(page: Page) {
  for (const selector of [
    ".masthead h1",
    ".banner h2",
    ".status-card__value",
  ]) {
    const ratio = await page
      .locator(selector)
      .first()
      .evaluate((element) => {
        const parse = (color: string) =>
          color
            .match(/[\d.]+/g)!
            .slice(0, 3)
            .map(Number);
        const luminance = (color: number[]) => {
          const channels = color.map((channel) => {
            const normalized = channel / 255;
            return normalized <= 0.04045
              ? normalized / 12.92
              : Math.pow((normalized + 0.055) / 1.055, 2.4);
          });
          return (
            0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2]
          );
        };
        const style = getComputedStyle(element);
        const foreground = luminance(parse(style.color));
        let backgroundElement: Element | null = element;
        let background = [8, 11, 14];
        while (backgroundElement) {
          const candidate = getComputedStyle(backgroundElement).backgroundColor;
          if (!candidate.endsWith(", 0)") && candidate !== "rgba(0, 0, 0, 0)") {
            background = parse(candidate);
            break;
          }
          backgroundElement = backgroundElement.parentElement;
        }
        const backgroundLuminance = luminance(background);
        return (
          (Math.max(foreground, backgroundLuminance) + 0.05) /
          (Math.min(foreground, backgroundLuminance) + 0.05)
        );
      });
    expect(ratio, `${selector} contrast`).toBeGreaterThanOrEqual(7);
  }
}
