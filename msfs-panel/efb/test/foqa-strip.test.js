// node msfs-panel/efb/test/foqa-strip.test.js
var assert = require("assert");
var S = require("../../PackageSources/html_ui/InGamePanels/FOQASim/foqa-strip.js");

function strip(over) {
    var s = { v: 2, id: "a1", key: "ltfj-06-v1", n: 2, stage: "flying", text: "Attempt 2 · Placed", can_fly_again: true, can_finish: true };
    for (var k in over || {}) s[k] = over[k];
    return s;
}

// Markup: the app's words, escaped; buttons by what the attempt allows.
var open = S.html(strip());
assert.ok(open.indexOf("Attempt 2 · Placed") > 0, open);
assert.ok(open.indexOf('data-strip="finish"') > 0 && open.indexOf('data-strip="fly_again"') > 0, open);
assert.ok(S.html(strip(), { confirm: true }).indexOf("Abandon &amp; fly again") > 0);
assert.ok(S.html(strip(), { busy: "fly_again" }).indexOf("Placing…") > 0);
assert.strictEqual(S.html(strip(), { busy: "finish" }).indexOf("data-strip"), -1, "no buttons while busy");
var scored = S.html(strip({ stage: "graded", can_finish: false, text: "Attempt 2 · Scored 87.4, #3", site: { state: "scored", score: 87.4, rank: 3 } }));
assert.ok(scored.indexOf('<div class="strip stage-graded ok">') === 0, scored);
assert.strictEqual(scored.indexOf('data-strip="finish"'), -1);
assert.ok(S.html(strip({ text: "<b>x</b>" })).indexOf("&lt;b&gt;x&lt;/b&gt;") > 0, "escaped");
assert.strictEqual(S.html(null), "");

// The Strip: renders into each mounted element, goes when the app is silent,
// asks once more before Fly again abandons an open attempt.
var t = 0, sent = [];
function el() { return { innerHTML: "", addEventListener: function (n, f) { this.click = f; } }; }
var bus = { send: function (name, json) { sent.push([name, JSON.parse(json)]); } };
var st = new S.Strip(bus, function () { return t; });
var a = el(), b = el();
st.mount(a); st.mount(b);
assert.strictEqual(a.innerHTML, "");
st.onAttempt(JSON.stringify(strip()));
assert.ok(a.innerHTML.indexOf("Placed") > 0 && a.innerHTML === b.innerHTML);
st.act("fly_again");
assert.strictEqual(sent.length, 0, "first click arms");
assert.ok(a.innerHTML.indexOf("Abandon &amp; fly again") > 0);
t = 1000;
st.act("fly_again");
assert.deepStrictEqual(sent[0], ["FOQA_SIM_CMD", { v: 2, cmd: "fly_again" }]);
st.act("finish");
assert.strictEqual(sent.length, 1, "one command at a time");
st.onAck({ cmd: "fly_again", ok: true });
st.act("finish");
assert.deepStrictEqual(sent[1][1], { v: 2, cmd: "finish" });
// A new stage clears the wait.
st.onAttempt(JSON.stringify(strip({ stage: "rollout", text: "Attempt 2 · Landed 142 fpm" })));
assert.ok(a.innerHTML.indexOf("Landed 142 fpm") > 0 && a.innerHTML.indexOf("Finishing") < 0);
// Over a closed attempt, Fly again goes at once.
st.onAttempt(JSON.stringify(strip({ stage: "graded", can_finish: false })));
st.act("fly_again");
assert.strictEqual(sent[2][1].cmd, "fly_again");
// Clicks reach it through the element.
st.onAck({ cmd: "fly_again", ok: false });
st.onAttempt(JSON.stringify(strip({ stage: "abandoned", can_finish: false })));
a.click({ target: { getAttribute: function () { return "fly_again"; } } });
assert.strictEqual(sent.length, 4);
t += 6000;
st.render();
assert.strictEqual(a.innerHTML, "", "stale: the strip goes");
console.log("foqa-strip ok");
