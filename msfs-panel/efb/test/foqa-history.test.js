// node msfs-panel/efb/test/foqa-history.test.js
var assert = require("assert");
var H = require("../foqa-history.js");

// A bar shades the share of its weight lost, not the points: with a domain n/a the points are rescaled above it.
var html = H.barsHtml({ bars: [[25, 14.7, 2, 0.5], [10, null, 0, 0], [15, 0, 0, 0]], top: "Approach stability −14.7: x" });
var lost = html.match(/class="mlost[^"]*" style="width:(\d+)%"/g);
assert.deepStrictEqual(lost, ['class="mlost lvl-medium" style="width:50%"']);
assert.ok(html.indexOf("−14.7") > 0 && html.indexOf("n/a") > 0, html);
assert.ok(html.indexOf('<div class="mtop">Approach stability −14.7: x</div>') > 0, html);
assert.strictEqual(H.barsHtml({}), "");
console.log("foqa-history ok");
