// FOQA-Sim attempt strip: the current challenge attempt as the app words it
// (FOQA_SIM_ATTEMPT, docs/challenge-protocol.md B9) with Finish and
// Fly again, rendered into any number of elements (the toolbar board, the
// EFB's Live and Challenges tabs). ES5 and DOM only, like foqa-board.js;
// the app writes every word, the strip only shows it.
// bus: { on(name, cb(json)), send(name, json) }.
var FoqaStrip = (function () {
    "use strict";

    var MSG = { attempt: "FOQA_SIM_ATTEMPT", cmd: "FOQA_SIM_CMD" };
    var STALE_MS = 5000;      // sent each second: none this long, the strip goes
    var CONFIRM_MS = 6000;    // Fly again over an open attempt waits this long for its second click
    var ACK_WAIT_MS = 15000;  // placing waits for the sim
    var BUSY = { fly_again: "Placing…", finish: "Finishing…", place: "Placing…", abandon: "Abandoning…" };
    var TONE = { scored: "ok", unverified: "warn", rejected: "bad", expired: "bad", signed_out: "warn" };

    function esc(s) {
        return String(s).replace(/[&<>"]/g, function (c) {
            return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c];
        });
    }

    // The strip's markup; ui: { busy: cmd or null, confirm: bool }.
    function html(s, ui) {
        if (!s) return "";
        ui = ui || {};
        var site = s.site ? s.site.state : "local", tone = TONE[site] || "";
        var out = '<div class="strip stage-' + esc(s.stage) + (tone ? " " + tone : "") + '">' +
            '<span class="stext">' + esc(s.text) + "</span>";
        if (ui.busy) return out + '<span class="sbusy">' + esc(BUSY[ui.busy] || "…") + "</span></div>";
        if (s.can_finish) out += '<span class="btn" data-strip="finish">Finish</span>';
        if (s.can_fly_again) {
            out += ui.confirm
                ? '<span class="btn danger armed" data-strip="fly_again">Abandon &amp; fly again</span>'
                : '<span class="btn primary" data-strip="fly_again">Fly again</span>';
        }
        return out + "</div>";
    }

    function Strip(bus, now) {
        this.bus = bus || {};
        this.now = now || function () { return Date.now(); };
        this.strip = null;
        this.at = 0;
        this.busy = null;      // { cmd, at }
        this.confirmAt = null;   // the first click on Fly again over an open attempt
        this.els = [];
    }

    // Also renders into `el`; clicks on its buttons act.
    Strip.prototype.mount = function (el) {
        var self = this;
        this.els.push({ el: el, html: null });
        el.addEventListener("click", function (e) {
            var t = e.target;
            while (t && t !== el && !(t.getAttribute && t.getAttribute("data-strip"))) t = t.parentNode;
            if (t && t !== el) self.act(t.getAttribute("data-strip"));
        });
        this.render();
    };

    Strip.prototype.current = function () {
        return this.strip && this.now() - this.at <= STALE_MS ? this.strip : null;
    };

    Strip.prototype.onAttempt = function (json) {
        var s = JSON.parse(json), was = this.strip;
        // Another attempt, or its stage moved: the command did its part.
        if (this.busy && (!was || was.id !== s.id || was.stage !== s.stage)) this.busy = null;
        this.strip = s;
        this.at = this.now();
        this.render();
    };

    // The app's ack of a command (from the board's status).
    Strip.prototype.onAck = function (ack) {
        if (this.busy && ack.cmd === this.busy.cmd) this.busy = null;
        this.render();
    };

    Strip.prototype.send = function (cmd, fields) {
        if (!this.bus.send || this.busy) return false;
        var msg = { v: 2, cmd: cmd }, k;
        for (k in fields || {}) msg[k] = fields[k];
        this.bus.send(MSG.cmd, JSON.stringify(msg));
        this.busy = { cmd: cmd, at: this.now() };
        this.render();
        return true;
    };

    Strip.prototype.act = function (act) {
        var s = this.current();
        if (!s || this.busy) return;
        if (act === "finish" && s.can_finish) this.send("finish");
        else if (act === "fly_again" && s.can_fly_again) {
            // Over an open attempt it abandons it: ask once more first.
            if (s.can_finish && !this.armed()) this.confirmAt = this.now();
            else { this.confirmAt = null; this.send("fly_again"); }
        }
        this.render();
    };

    Strip.prototype.armed = function () {
        return this.confirmAt !== null && this.now() - this.confirmAt <= CONFIRM_MS;
    };

    Strip.prototype.render = function () {
        var t = this.now();
        if (this.busy && t - this.busy.at > ACK_WAIT_MS) this.busy = null;
        var markup = html(this.current(), {
            busy: this.busy && this.busy.cmd,
            confirm: this.armed()
        });
        this.els.forEach(function (m) {
            if (m.html === markup) return;
            m.html = markup;
            m.el.innerHTML = markup;
        });
    };

    function attach(bus, now) {
        var strip = new Strip(bus, now);
        bus.on(MSG.attempt, function (json) { strip.onAttempt(json); });
        if (typeof setInterval === "function") setInterval(function () { strip.render(); }, 1000);
        return strip;
    }

    var api = { MSG: MSG, attach: attach, Strip: Strip, html: html, esc: esc };
    if (typeof module !== "undefined" && module.exports) module.exports = api;
    return api;
})();
