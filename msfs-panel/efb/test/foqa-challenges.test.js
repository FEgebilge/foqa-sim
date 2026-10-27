// node msfs-panel/efb/test/foqa-challenges.test.js
var assert = require("assert");
global.FoqaStrip = require("../../PackageSources/html_ui/InGamePanels/FOQASim/foqa-strip.js");
var C = require("../foqa-challenges.js");

var card = { key: "ltfj-06-v1", title: "LTFJ 06 <gusty>", airport: "LTFJ", runway: "06", source: "Pilot Aptitude (pilotaptitude.example)",
    state: "open", last: "Attempt 3 · Scored 81.0, #4", rank: 4 };
var list = C.listHtml([card, { key: "b-v1", title: "B", airport: "TEST", runway: "20", source: "File", state: "closed" }]);
assert.ok(list.indexOf('data-act="open:ltfj-06-v1"') > 0, list);
assert.ok(list.indexOf("LTFJ 06 &lt;gusty&gt;") > 0 && list.indexOf("· #4") > 0, list);
assert.ok(list.indexOf("File · Closed") > 0, list);
assert.ok(C.listHtml([]).indexOf("No challenges yet") > 0);

function briefing(states) {
    return { v: 2, req: "c1", key: "ltfj-06-v1", title: "LTFJ 06", lines: ["Start: 8 NM out", "Gate: 3 NM"],
        preflight: states.map(function (s, i) { return { name: ["sim", "aircraft", "weather", "ranking"][i], state: s, text: "t" + i, fix: s === "fail" ? "fix" + i : undefined }; }) };
}
var ok = C.briefingHtml(briefing(["ok", "ok", "unknown", "ok"]));
assert.ok(ok.indexOf('data-act="place"') > 0 && ok.indexOf('primary off') < 0, ok);
assert.ok(ok.indexOf("Start: 8 NM out") > 0 && ok.indexOf("Simulator") > 0);
var aircraft = C.briefingHtml(briefing(["ok", "fail", "ok", "ok"]));
assert.ok(aircraft.indexOf("primary off") > 0 && aircraft.indexOf('class="btn wide" data-act="practice"') > 0, "practice only");
assert.ok(aircraft.indexOf("fix1") > 0);
var noSim = C.briefingHtml(briefing(["fail", "unknown", "unknown", "ok"]));
assert.ok(noSim.indexOf('btn wide off" data-act="practice"') > 0, "nothing to place without the sim");
assert.ok(C.briefingHtml(briefing(["ok"]), true).indexOf("Placing…") > 0);
assert.ok(C.briefingHtml({ error: "no challenge \"x\"" }).indexOf("no challenge &quot;x&quot;") > 0);

// The tab: asks, takes only its own reply, places, re-asks the briefing.
var t = 0, sent = [];
function node() { return { innerHTML: "", addEventListener: function () {}, querySelector: function () { return node(); }, classList: { add: function () {} } }; }
var root = node(), body = node();
root.querySelector = function (sel) { return sel === ".body" ? body : node(); };
var bus = { send: function (name, json) { sent.push(JSON.parse(json)); } };
var tab = new C.Challenges(root, bus, null, function () { return t; });
tab.show();
assert.strictEqual(sent[0].cmd, "list_challenges");
tab.onChallenges(JSON.stringify({ v: 2, req: "other", challenges: [card] }));
assert.ok(body.innerHTML.indexOf("Asking") >= 0, "another view's reply is ignored");
tab.onChallenges(JSON.stringify({ v: 2, req: sent[0].req, challenges: [card] }));
assert.ok(body.innerHTML.indexOf("LTFJ 06") > 0);
tab.act("open:ltfj-06-v1");
assert.deepStrictEqual([sent[1].cmd, sent[1].key], ["briefing", "ltfj-06-v1"]);
var b = briefing(["ok", "ok", "ok", "ok"]); b.req = sent[1].req;
tab.onBriefing(JSON.stringify(b));
assert.ok(body.innerHTML.indexOf("Pre-flight check") > 0);
tab.act("practice");
assert.deepStrictEqual(sent[2], { v: 2, cmd: "place", key: "ltfj-06-v1", practice: true });
tab.act("place");
assert.strictEqual(sent.length, 3, "one place at a time");
tab.onAck({ cmd: "place", ok: false, message: "MSFS isn't connected." });
assert.ok(body.innerHTML.indexOf("MSFS isn't connected.") > 0);
t = 5001;
tab.tick();
assert.strictEqual(sent[3].cmd, "briefing", "asked again while shown");
tab.hide();
t = 20000;
tab.tick();
assert.ok(body.innerHTML.indexOf("No reply from the FOQA-Sim app") > 0);
assert.strictEqual(sent.length, 4, "hidden: not asked again");
console.log("foqa-challenges ok");
