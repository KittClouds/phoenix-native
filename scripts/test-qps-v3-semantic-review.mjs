import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const html = readFileSync(
  new URL("../docs/qps-v3-semantic-review.html", import.meta.url),
  "utf8",
);
const scriptMatch = html.match(/<script>([\s\S]*?)<\/script>/u);
assert(scriptMatch, "reviewer script is present");

class FakeClassList {
  names = new Set();

  toggle(name, force) {
    const enabled = force ?? !this.names.has(name);
    if (enabled) this.names.add(name);
    else this.names.delete(name);
    return enabled;
  }
}

class FakeElement {
  constructor(id) {
    this.id = id;
    this.classList = new FakeClassList();
    this.disabled = false;
    this.files = [];
    this.innerHTML = "";
    this.textContent = "";
    this.value = "";
  }

  click() {
    return this.onclick?.({ target: this });
  }
}

function createHarness() {
  const ids = [...html.matchAll(/id="([A-Za-z][A-Za-z0-9]*)"/gu)].map(
    (match) => match[1],
  );
  const elements = Object.fromEntries(ids.map((id) => [id, new FakeElement(id)]));
  elements.filter.value = "undecided";
  elements.reasonFilter.value = "all";
  elements.confidence.value = "90";
  elements.reviewer.value = "phoenix-human-curator";

  const storage = new Map();
  const localStorage = {
    getItem: (key) => storage.get(key) ?? null,
    removeItem: (key) => storage.delete(key),
    setItem: (key, value) => storage.set(key, value),
  };
  const document = {
    getElementById: (id) => elements[id] ?? null,
    onkeydown: null,
  };
  const alerts = [];
  const run = new Function(
    "document",
    "localStorage",
    "alert",
    "confirm",
    "Blob",
    "URL",
    scriptMatch[1],
  );
  run(document, localStorage, (message) => alerts.push(message), () => true, Blob, URL);
  return { alerts, document, elements };
}

function reviewBatch() {
  const document = (id, text) => ({
    id,
    reviewer_context: "",
    source_time_label: "2026-08-08",
    text,
    title: id,
  });
  const item = (identity, query) => ({
    dataset: "smoke",
    judgment_identity: identity,
    negative: document(`${identity}-negative`, "negative text"),
    negative_v2_position: 0,
    positive: document(`${identity}-positive`, "positive text"),
    positive_v2_position: 1,
    query,
    reference_answer: "reference",
    suggested_reason: "phrase_order_failure",
  });
  return {
    contract: "phoenix.qps.semantic-review-batch/v2",
    excluded_batch: { sha256: "excluded" },
    items: [item("one", "first query"), item("two", "second query")],
    source_packet: { sha256: "source" },
  };
}

async function load(harness, batch = reviewBatch()) {
  await harness.elements.batchFile.onchange({
    target: { files: [{ text: async () => JSON.stringify(batch) }] },
  });
}

const normal = createHarness();
await load(normal);
assert.equal(normal.elements.query.textContent, "first query");
assert.equal(normal.elements.prev.disabled, true);
assert.equal(normal.elements.next.disabled, false);
normal.elements.next.click();
assert.equal(normal.elements.query.textContent, "second query");
assert.equal(normal.elements.next.disabled, true);

const earlyNavigation = createHarness();
earlyNavigation.elements.next.click();
await load(earlyNavigation);
assert.equal(earlyNavigation.elements.query.textContent, "first query");
assert.equal(earlyNavigation.elements.stats.textContent, "0/2 reviewed · 0 exportable · 0 abstain · 2 shown");

const decision = createHarness();
await load(decision);
decision.elements.chooseP.click();
assert.equal(decision.elements.query.textContent, "second query");
assert.equal(decision.elements.stats.textContent, "1/2 reviewed · 1 exportable · 0 abstain · 1 shown");

const filtered = createHarness();
await load(filtered);
filtered.elements.filter.value = "positive_preferred";
filtered.elements.filter.onchange();
assert.match(filtered.elements.empty.textContent, /Completed.*already made/u);
assert.equal(filtered.elements.stats.textContent, "0/2 reviewed · 0 exportable · 0 abstain · 0 shown");
assert.equal(filtered.elements.next.disabled, true);
filtered.elements.resetFilters.click();
assert.equal(filtered.elements.filter.value, "undecided");
assert.equal(filtered.elements.reasonFilter.value, "all");
assert.equal(filtered.elements.query.textContent, "first query");

if (process.argv[2]) {
  const actualBatch = JSON.parse(readFileSync(process.argv[2], "utf8"));
  assert(actualBatch.items.length > 1, "actual batch contains navigable items");
  const actual = createHarness();
  await load(actual, actualBatch);
  assert.equal(actual.elements.query.textContent, actualBatch.items[0].query);
  actual.elements.next.click();
  assert.equal(actual.elements.query.textContent, actualBatch.items[1].query);
  assert.equal(actual.alerts.length, 0);
  console.log(`QPS_V3_ACTUAL_BATCH_NEXT_OK (${actualBatch.items.length} items)`);
}

console.log("QPS_V3_SEMANTIC_REVIEW_SMOKE_OK");
