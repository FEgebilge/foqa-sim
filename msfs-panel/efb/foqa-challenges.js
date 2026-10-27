// FOQA-Sim EFB Challenges tab: the app's challenge library (list_challenges),
// one challenge's briefing with the pre-flight card (briefing, asked again
// every 5 s while shown), Place / Fly as practice, and the current attempt's
// strip with Fly again (docs/challenge-protocol.md B9). ES5 and DOM only;
// every word comes from the app. Load foqa-strip.js and foqa-board.js first.
// bus: { on(name, cb(json)), send(name, json) }.
var FoqaChallenges = (function () {
    "use strict";

    var MSG = { challenges: "FOQA_SIM_CHALLENGES", briefing: "FOQA_SIM_BRIEFING", cmd: "FOQA_SIM_CMD" };
    var REPLY_WAIT_MS = 8000;   // the app repeats a reply for ~5 s
    var REASK_MS = 5000;        // the briefing's pre-flight card, while shown
    var STATE_TEXT = { open: "", upcoming: "Upcoming", closed: "Closed", needs_update: "Needs a newer FOQA-Sim" };
    var CHECK_NAMES = { sim: "Simulator", aircraft: "Aircraft", weather: "Weather", ranking: "Ranking" };
    var esc = FoqaStrip.esc;
    var reqs = 0;

    function cardHtml(c) {
        var state = STATE_TEXT[c.state];
        return '<div class="ccard" data-act="open:' + esc(c.key) + '">' +
            '<div class="crow"><span class="ctitle">' + esc(c.title) + "</span>" +
            '<span class="cwhere">' + esc(c.airport + " " + c.runway) + "</span></div>" +
            '<div class="csub">' + esc(c.source) + (state ? " · " + esc(state) : "") +
            (typeof c.rank === "number" ? " · #" + c.rank : "") + "</div>" +
            (c.last ? '<div class="clast">' + esc(c.last) + "</div>" : "") + "</div>";
    }

    // The list: cards, newest flown first, as the app sends them.
    function listHtml(cards) {
        if (!cards.length) {
            return '<div class="note">No challenges yet. Open a challenge file in the FOQA-Sim app on this PC, ' +
                "or take one from a site; it appears here.</div>";
        }
        return cards.map(cardHtml).join("");
    }

    function checkHtml(p) {
        return '<div class="check ' + esc(p.state) + '"><span class="cname">' + esc(CHECK_NAMES[p.name] || p.name) + "</span>" +
            '<span class="ctext">' + esc(p.text) + (p.fix ? '<span class="cfix">' + esc(p.fix) + "</span>" : "") + "</span></div>";
    }

    // One challenge: its facts, the pre-flight card, and Place unless a
    // check fails (then only as practice, which is never ranked).
    function briefingHtml(b, busy) {
        if (b.error) return '<div class="note">' + esc(b.error) + "</div>";
        var failing = b.preflight.some(function (p) { return p.state === "fail"; });
        var sim = b.preflight.filter(function (p) { return p.name === "sim"; })[0];
        var simDown = sim && sim.state === "fail";
        var html = '<div class="bhead"><span class="btn" data-act="back">‹ All</span><span class="btitle">' + esc(b.title) + "</span></div>" +
            '<div class="blines">' + b.lines.map(function (l) { return '<div class="bline">' + esc(l) + "</div>"; }).join("") + "</div>" +
            '<div class="section">Pre-flight check</div>' + b.preflight.map(checkHtml).join("");
        html += '<div class="actions">';
        if (busy) html += '<span class="btn wide off">Placing…</span>';
        else {
            html += '<span class="btn wide primary' + (failing ? " off" : "") + '" data-act="place">Place</span>' +
                '<span class="btn wide' + (simDown ? " off" : "") + '" data-act="practice">Fly as practice</span>';
        }
        return html + '</div><div class="hint">' + (failing
            ? (simDown ? "Load a flight in MSFS first." : "Fix the failing check to place a ranked attempt, or fly it as practice: graded here, never uploaded.")
            : "Place moves the aircraft to the start; landing grades the attempt.") + "</div>";
    }

    function Challenges(root, bus, strip, now) {
        this.root = root;
        this.bus = bus;
        this.now = now || function () { return Date.now(); };
        this.cards = null;
        this.briefing = null;   // the last briefing of `open`
        this.briefingAt = 0;
        this.open = null;       // key shown, null: the list
        this.wait = null;       // { req, cmd, at }
        this.error = null;
        this.placing = null;    // { at }
        this.shown = false;
        this.html = null;
        root.classList.add("foqa");
        root.classList.add("challenges");
        root.innerHTML = '<div class="cstrip"></div><div class="body"></div>';
        this.body = root.querySelector(".body");
        if (strip) strip.mount(root.querySelector(".cstrip"));
        var self = this;
        root.addEventListener("click", function (e) {
            var el = e.target;
            while (el && el !== root && !(el.getAttribute && el.getAttribute("data-act"))) el = el.parentNode;
            if (el && el !== root) self.act(el.getAttribute("data-act"));
        });
    }

    Challenges.prototype.ask = function (cmd, key) {
        var req = "c" + (++reqs) + "-" + this.now().toString(36), msg = { v: 2, cmd: cmd, req: req };
        if (key) msg.key = key;
        this.wait = { req: req, cmd: cmd, at: this.now() };
        if (this.bus.send) this.bus.send(MSG.cmd, JSON.stringify(msg));
        this.render();
    };

    // The tab is shown: the list fresh, or the briefing asked again.
    Challenges.prototype.show = function () {
        this.shown = true;
        if (this.open) this.ask("briefing", this.open);
        else this.ask("list_challenges");
    };

    Challenges.prototype.hide = function () { this.shown = false; };

    Challenges.prototype.onChallenges = function (json) {
        var msg = JSON.parse(json);
        if (!this.wait || msg.req !== this.wait.req) return;   // a repeat, or another view's
        this.wait = null;
        this.error = null;
        this.cards = msg.challenges || [];
        this.render();
    };

    Challenges.prototype.onBriefing = function (json) {
        var msg = JSON.parse(json);
        if (!this.wait || msg.req !== this.wait.req || msg.key !== this.open) return;
        this.wait = null;
        this.error = null;
        this.briefing = msg;
        this.briefingAt = this.now();
        this.render();
    };

    // The app's ack of Place.
    Challenges.prototype.onAck = function (ack) {
        if (ack.cmd !== "place" || !this.placing) return;
        this.placing = null;
        if (!ack.ok) this.error = ack.message || "Could not place the aircraft";
        this.render();
    };

    Challenges.prototype.act = function (act) {
        if (act.indexOf("open:") === 0) {
            this.open = act.slice(5);
            this.briefing = null;
            this.ask("briefing", this.open);
        } else if (act === "back") {
            this.open = null;
            this.ask("list_challenges");
        } else if ((act === "place" || act === "practice") && this.open && !this.placing && this.bus.send) {
            this.bus.send(MSG.cmd, JSON.stringify({ v: 2, cmd: "place", key: this.open, practice: act === "practice" }));
            this.placing = { at: this.now() };
            this.error = null;
            this.render();
        } else if (act === "retry") this.show();
    };

    // Each tick: timeouts, and the briefing's card asked again while shown.
    Challenges.prototype.tick = function () {
        var t = this.now();
        if (this.placing && t - this.placing.at > REPLY_WAIT_MS * 2) {
            this.placing = null;
            this.error = "No reply from the FOQA-Sim app";
        }
        if (this.wait && t - this.wait.at > REPLY_WAIT_MS) {
            this.wait = null;
            this.error = "No reply from the FOQA-Sim app. Is it running on this PC?";
        }
        var stale = this.briefing && t - this.briefingAt > REASK_MS;
        if (this.shown && this.open && !this.wait && (stale || !this.briefing)) this.ask("briefing", this.open);
        this.render();
    };

    Challenges.prototype.render = function () {
        var html;
        if (this.open && this.briefing) html = briefingHtml(this.briefing, !!this.placing);
        else if (!this.open && this.cards) html = listHtml(this.cards);
        else if (this.wait) html = '<div class="note">Asking the FOQA-Sim app…</div>';
        else html = "";
        if (this.error) {
            html = '<div class="toast fail" data-act="retry">' + esc(this.error) + " Tap to try again.</div>" + html;
        }
        if (html === this.html) return;
        this.html = html;
        this.body.innerHTML = html;
    };

    function attach(root, bus, strip, now) {
        var c = new Challenges(root, bus, strip, now);
        bus.on(MSG.challenges, function (json) { c.onChallenges(json); });
        bus.on(MSG.briefing, function (json) { c.onBriefing(json); });
        if (typeof setInterval === "function") setInterval(function () { c.tick(); }, 1000);
        c.render();
        return c;
    }

    var api = { MSG: MSG, attach: attach, Challenges: Challenges, listHtml: listHtml, briefingHtml: briefingHtml };
    if (typeof module !== "undefined" && module.exports) module.exports = api;
    return api;
})();
