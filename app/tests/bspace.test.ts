// The experiment editor's value parsing.
import { test } from "node:test";
import assert from "node:assert/strict";
import { splitLiterals, steppedCount } from "../src/bspace";

test("values split as NetLogo literals", () => {
  assert.deepEqual(splitLiterals("1 2.5  -3"), ["1", "2.5", "-3"]);
  assert.deepEqual(splitLiterals('"zones" "water sources" true'), ['"zones"', '"water sources"', "true"]);
  assert.deepEqual(splitLiterals('"say \\"hi\\"" [1 [2 "]"] 3] x'), ['"say \\"hi\\""', '[1 [2 "]"] 3]', "x"]);
  assert.deepEqual(splitLiterals("   "), []);
});

test("unclosed strings and lists are reported", () => {
  assert.equal(typeof splitLiterals('"open'), "string");
  assert.equal(typeof splitLiterals("[1 2"), "string");
  assert.equal(typeof splitLiterals("1 ]"), "string");
});

test("stepped value counts", () => {
  assert.equal(steppedCount("0", "0.1", "0.3"), 4);
  assert.equal(steppedCount("1", "1", "8"), 8);
  assert.equal(steppedCount("5", "1", "1"), 0);
  assert.equal(steppedCount("0", "0", "1"), null);
});
