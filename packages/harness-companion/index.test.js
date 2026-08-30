import assert from "node:assert/strict";
import test from "node:test";

import { queryStatus } from "./index.js";

test("prefers the desktop control snapshot", async () => {
  const fetch = async (url) => {
    assert.match(String(url), /\/v1\/status$/);
    return {
      ok: true,
      json: async () => ({
        ollama: { state: "online" },
        harness: { state: "online" },
        runningModels: [{ name: "qwen", sizeVram: 8 * 1024 ** 3 }],
      }),
    };
  };
  const value = await queryStatus({}, fetch);
  assert.match(value, /desktop: connected/);
  assert.match(value, /8\.0 GB VRAM/);
});

test("falls back to the read-only Ollama endpoint", async () => {
  const fetch = async (url) => {
    if (String(url).endsWith("/v1/status")) throw new Error("desktop offline");
    return {
      ok: true,
      json: async () => ({ models: [{ name: "qwen", size_vram: 4 * 1024 ** 3 }] }),
    };
  };
  const value = await queryStatus({}, fetch);
  assert.match(value, /desktop: not connected/);
  assert.match(value, /qwen: 4\.0 GB VRAM/);
});

test("rejects non-loopback configuration", async () => {
  await assert.rejects(
    queryStatus({ ollamaUrl: "https://example.com" }, async () => {
      throw new Error("must not fetch");
    }),
    /loopback URL/,
  );
});

