// FOQA-Sim board: renders what the FOQA-Sim app sends over the Comm Bus
// (docs/ingame-panel.md) into one element and sends the pilot's commands
// back. Plain ES2015 and DOM only, no sim APIs: the toolbar panel, a later
// EFB app and the dev page share it through a bus { on(name, cb), send(name, json) }.
var FoqaSim = (function () {
    "use strict";

    var MSG = {
        status: "FOQA_SIM_STATUS", live: "FOQA_SIM_LIVE", events: "FOQA_SIM_EVENTS",
        landing: "FOQA_SIM_LANDING", landings: "FOQA_SIM_LANDINGS", cmd: "FOQA_SIM_CMD"
    };
    var FRESH_S = 120;          // a landing at most this old opens the Landings tab by itself
    var STATUS_STALE_MS = 4000; // no status this long: the app is not running or not connected
    var LIVE_STALE_MS = 2000;
    var FLASH_MS = 8000;        // a new event stays in the banner this long
    var TOAST_MS = 6000;
    var CONFIRM_MS = 6000;      // a discard waits this long for its second click
    var ACK_WAIT_MS = 8000;
    var TOP_EVENTS = 3;
    var LEVELS = ["high", "medium", "low", "advisory"]; // most severe first
    var PHASE_NAMES = { InitialClimb: "Initial climb", EnRoute: "En route" };
    var APPROACH = { Approach: true, Landing: true };
    var TABS = [["now", "Now"], ["events", "Events"], ["landings", "Landings"], ["flight", "Flight"]];
    var CMD_TEXT = {
        start_flight: { busy: "Starting…", ok: "Recording a new flight", fail: "Could not start a flight" },
        end_flight: { busy: "Saving…", ok: "Flight saved", fail: "Could not save the flight" },
        discard_flight: { busy: "Discarding…", ok: "Flight discarded", fail: "Could not discard the flight" },
        mark: { busy: "Marking…", ok: "Marked: it is in the debrief", fail: "Could not mark" },
        // Challenge attempts (the strip, foqa-strip.js, and the EFB's Challenges tab).
        place: { ok: "Placed at the challenge's start", fail: "Could not place the aircraft" },
        fly_again: { ok: "New attempt placed", fail: "Could not fly again" },
        finish: { ok: "Attempt finished: grading", fail: "Could not finish the attempt" },
        abandon: { ok: "Attempt abandoned", fail: "Could not abandon the attempt" }
    };

    function esc(s) {
        return String(s).replace(/[&<>"]/g, function (c) {
            return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c];
        });
    }

    function isNum(v) { return typeof v === "number" && isFinite(v); }

    function num(v, digits) { return isNum(v) ? v.toFixed(digits || 0) : "–"; }

    function cap(s) { return s ? s.charAt(0).toUpperCase() + s.slice(1) : ""; }

    function clock(s) {
        s = Math.max(0, Math.floor(s));
        var h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, sec = s % 60;
        var mm = (h ? (m < 10 ? "0" : "") : "") + m;
        return (h ? h + ":" : "") + mm + ":" + (sec < 10 ? "0" : "") + sec;
    }

    // "109 kt", "20°", "1.27 g".
    function qty(v, unit) {
        var digits = unit === "g" || unit === "dot" ? 2 : 0;
        return num(v, digits) + (unit === "deg" ? "°" : unit ? " " + unit : "");
    }

    // Most severe first, then latest.
    function bySeverity(a, b) { return LEVELS.indexOf(a.level) - LEVELS.indexOf(b.level) || b.start_s - a.start_s; }

    function worstOf(events) {
        return events.reduce(function (w, e) { return !w || LEVELS.indexOf(e.level) < LEVELS.indexOf(w) ? e.level : w; }, null);
    }

    function checkText(c) {
        return cap(c.name) + (isNum(c.value) ? " " + qty(c.value, c.unit) : "") +
            (isNum(c.limit) ? ", limit " + qty(c.limit, c.unit) : "");
    }

    // What the Landings tab shows of a LandingNotice.
    function landingView(notice) {
        var td = notice.touchdown, rw = td.runway;
        var events = notice.events.slice().sort(bySeverity);
        return {
            key: notice.flight_id + "@" + td.t_s,
            flight: notice.flight_id,
            t: td.t_s,
            sink: num(td.sink_fpm, 0),
            g: num(td.peak_g, 2),
            ias: isNum(td.ias_kt) ? num(td.ias_kt, 0) : null,
            bounce: td.bounce,
            runway: rw ? rw.icao + " " + rw.runway : null,
            along: rw ? num(rw.along_m, 0) + " m" : null,
            cross: rw ? num(Math.abs(rw.cross_m), 1) + " m " + (rw.cross_m < 0 ? "left" : "right") : null,
            zone: rw ? rw.zone : null,
            zoneOk: rw ? rw.in_tdz : null,
            worst: worstOf(events),
            events: events.slice(0, TOP_EVENTS).map(function (e) {
                return { level: e.level, title: e.title, detail: e.detail };
            }),
            more: Math.max(0, events.length - TOP_EVENTS)
        };
    }

    // A section redrawn between press and release loses the click, so
    // view.render() draws nothing while a button is held on root
    // (view.pressing). A release anywhere, or the mouse leaving root, ends
    // the hold.
    function holdRedraws(root, view) {
        function release() {
            if (!view.pressing) return;
            view.pressing = false;
            setTimeout(function () { view.render(); }, 0);
        }
        view.pressing = false;
        root.addEventListener("mousedown", function () { view.pressing = true; });
        root.addEventListener("mouseleave", release);
        document.addEventListener("mouseup", release);
    }

    function Board(root, bus, now) {
        this.root = root;
        this.bus = bus || {};
        this.now = now || function () { return Date.now(); };
        this.status = null;      // last Status message
        this.statusAt = 0;
        this.live = null;
        this.liveAt = 0;
        this.events = null;      // last Events message
        this.seen = null;        // event keys already heard; null until the first Events
        this.flashes = [];       // { event, at }
        this.landings = [];      // landing views, newest first
        this.tab = "now";
        this.backTab = null;     // where × on an auto-opened landing returns
        this.pending = null;     // { cmd, at }: sent, no ack yet
        this.confirmAt = 0;      // first click on Discard
        this.toast = null;       // { ok, text, at }
        this.cache = {};
        this.ackHooks = [];      // also told each ack: the strip, the Challenges tab
        root.classList.add("foqa");
        root.innerHTML = '<div class="top"></div><div class="flash-slot"></div><div class="tabs"></div>' +
            '<div class="body"></div><div class="strip-slot"></div><div class="toast-slot"></div>';
        this.slots = {};
        var self = this;
        ["top", "flash-slot", "tabs", "body", "toast-slot"].forEach(function (c) { self.slots[c] = root.querySelector("." + c); });
        holdRedraws(root, this);
        root.addEventListener("click", function (e) {
            var el = e.target;
            while (el && el !== root && !el.getAttribute("data-act")) el = el.parentNode;
            if (el && el !== root) self.act(el.getAttribute("data-act"));
        });
        setInterval(function () { self.render(); }, 500);
        this.render();
    }

    Board.prototype.recording = function () {
        return !this.stale() && this.status.status.state === "recording";
    };

    // Connected to the sim with no flight open: one can be started.
    Board.prototype.ready = function () {
        return !this.stale() && this.status.status.state === "connected";
    };

    Board.prototype.stale = function () {
        return !this.status || this.now() - this.statusAt > STATUS_STALE_MS;
    };

    Board.prototype.freshLive = function () {
        return this.live && this.now() - this.liveAt <= LIVE_STALE_MS ? this.live : null;
    };

    Board.prototype.onStatus = function (json) {
        var msg = JSON.parse(json);
        this.status = msg;
        this.statusAt = this.now();
        if (msg.ack) {
            var t = CMD_TEXT[msg.ack.cmd] || { ok: "Done", fail: "Failed" }, gone = this.pending && this.pending.flight;
            // A discarded flight leaves nothing behind on the board either.
            if (msg.ack.cmd === "discard_flight" && msg.ack.ok && gone) {
                this.landings = this.landings.filter(function (l) { return l.flight !== gone; });
                this.events = null;
                this.flashes = [];
            }
            this.toast = { ok: msg.ack.ok, text: msg.ack.message || (msg.ack.ok ? t.ok : t.fail), at: this.now() };
            if (this.pending && this.pending.cmd === msg.ack.cmd) this.pending = null;
            this.ackHooks.forEach(function (f) { f(msg.ack); });
        }
        this.render();
    };

    Board.prototype.onLive = function (json) {
        var live = JSON.parse(json), was = this.live && this.live.phase;
        this.live = live;
        this.liveAt = this.now();
        // Into the approach: show the approach monitor.
        if (APPROACH[live.phase] && !APPROACH[was] && this.tab !== "now") { this.tab = "now"; this.backTab = null; }
        this.render();
    };

    Board.prototype.onEvents = function (json) {
        var msg = JSON.parse(json), self = this, t = this.now(), first = !this.seen;
        if (first) this.seen = {};
        msg.events.forEach(function (e) {
            var key = msg.flight_id + "@" + e.rule + "@" + e.start_s;
            if (self.seen[key]) return;
            self.seen[key] = true;
            // Events already there when the panel opened are not news.
            if (!first) self.flashes.push({ event: e, at: t });
        });
        this.events = msg;
        this.render();
    };

    Board.prototype.onLanding = function (json) {
        var msg = JSON.parse(json), view = landingView(msg.notice);
        var isNew = !this.landings.some(function (l) { return l.key === view.key; });
        this.mergeLandings([view]);
        if (isNew && msg.age_s <= FRESH_S) {
            if (this.tab !== "landings") this.backTab = this.tab;
            this.tab = "landings";
        }
        this.render();
    };

    Board.prototype.onLandings = function (json) {
        this.mergeLandings(JSON.parse(json).landings.map(landingView));
        this.render();
    };

    // Newest 3 of the newest flight's landings.
    Board.prototype.mergeLandings = function (views) {
        var all = views.concat(this.landings.filter(function (l) {
            return !views.some(function (v) { return v.key === l.key; });
        }));
        var flight = views.length ? views[0].flight : null;
        this.landings = all.filter(function (l) { return l.flight === flight; })
            .sort(function (a, b) { return b.t - a.t; }).slice(0, 3);
    };

    Board.prototype.send = function (cmd) {
        var rec = this.recording();
        if (!this.bus.send || this.pending || !(cmd === "start_flight" ? this.ready() : rec)) return;
        this.bus.send(MSG.cmd, JSON.stringify({ v: 2, cmd: cmd }));
        this.pending = { cmd: cmd, at: this.now(), flight: rec ? this.status.status.flight_id : null };
    };

    Board.prototype.act = function (act) {
        if (act.indexOf("tab:") === 0) { this.tab = act.slice(4); this.backTab = null; }
        else if (act === "back") { this.tab = this.backTab || "now"; this.backTab = null; }
        else if (act === "start") this.send("start_flight");
        else if (act === "mark") this.send("mark");
        else if (act === "end") this.send("end_flight");
        else if (act === "discard") {
            if (this.now() - this.confirmAt <= CONFIRM_MS) { this.confirmAt = 0; this.send("discard_flight"); }
            else this.confirmAt = this.now();
        } else if (act === "cancel") this.confirmAt = 0;
        else if (act === "toast") this.toast = null;
        else return;
        this.render();
    };

    Board.prototype.render = function () {
        var t = this.now();
        if (this.pending && t - this.pending.at > ACK_WAIT_MS) {
            this.pending = null;
            this.toast = { ok: false, text: "No reply from the FOQA-Sim app", at: t };
        }
        if (this.toast && t - this.toast.at > TOAST_MS) this.toast = null;
        this.flashes = this.flashes.filter(function (f) { return t - f.at <= FLASH_MS; });
        if (this.pressing) return;
        var w = this.root.clientWidth, h = this.root.clientHeight;
        this.root.classList.toggle("narrow", w > 0 && w < 360);
        this.root.classList.toggle("short", h > 0 && h < 320);
        var body = this.slots.body, scroll = this.cache.tab === this.tab ? body.scrollTop : 0;
        this.cache.tab = this.tab;
        this.put("top", this.topHtml());
        this.put("flash-slot", this.flashHtml());
        this.put("tabs", this.tabsHtml());
        this.put("body", '<div class="pane-' + this.tab + '">' + this[this.tab + "Html"]() + "</div>");
        this.put("toast-slot", this.toast
            ? '<div class="toast ' + (this.toast.ok ? "ok" : "fail") + '" data-act="toast">' + esc(this.toast.text) + "</div>" : "");
        body.scrollTop = scroll;
    };

    // Only sections whose markup changed are redrawn, so animations play once.
    Board.prototype.put = function (slot, html) {
        if (this.cache[slot] === html) return;
        this.cache[slot] = html;
        this.slots[slot].innerHTML = html;
    };

    Board.prototype.phase = function () {
        var live = this.freshLive(), f = this.status && this.status.flight;
        var p = live ? live.phase : f && f.phase;
        return p ? { id: p, name: PHASE_NAMES[p] || p, hot: !!APPROACH[p] } : null;
    };

    Board.prototype.topHtml = function () {
        var s = this.stale() ? null : this.status.status, f = this.status && this.status.flight, st;
        if (!s) st = { cls: "off", text: "FOQA-Sim app not connected" };
        else if (s.state === "recording") st = { cls: "rec", text: "REC " + clock(f ? f.elapsed_s : s.elapsed_s) };
        else if (s.state === "connected") st = { cls: "idle", text: "Ready · no flight" };
        else if (s.state === "waiting_for_sim") st = { cls: "off", text: "Waiting for sim" };
        else st = { cls: "err", text: "Recorder error" };
        var html = '<span class="dot ' + st.cls + '"></span><span class="state">' + esc(st.text) + "</span>";
        var info = s && s.state === "recording" && f ? [f.aircraft, f.departure].filter(Boolean).join(" · ") : "";
        html += '<span class="sub">' + esc(info) + "</span>";
        var p = s && s.state === "recording" ? this.phase() : null;
        if (p) html += '<span class="phase' + (p.hot ? " hot" : "") + '">' + esc(p.name) + "</span>";
        if (s && s.state === "recording") {
            html += '<span class="btn mark' + (this.pending ? " off" : "") + '" data-act="mark">' +
                (this.pending && this.pending.cmd === "mark" ? "…" : "Mark") + "</span>";
        }
        return html;
    };

    Board.prototype.flashHtml = function () {
        if (!this.flashes.length) return "";
        var f = this.flashes.slice().sort(function (a, b) { return bySeverity(a.event, b.event); })[0], e = f.event;
        return '<div class="flash lvl-' + e.level + '" data-act="tab:events"><span class="lvl">' + e.level + "</span>" +
            '<span class="name">' + esc(e.title) + "</span>" +
            (e.detail ? '<span class="detail">' + esc(e.detail) + "</span>" : "") +
            (this.flashes.length > 1 ? '<span class="more">+' + (this.flashes.length - 1) + " new</span>" : "") + "</div>";
    };

    Board.prototype.tabsHtml = function () {
        var self = this, ev = this.events ? this.events.events : [];
        return TABS.map(function (tab) {
            var badge = "";
            if (tab[0] === "events" && ev.length) badge = '<span class="badge lvl-' + worstOf(ev) + '">' + ev.length + "</span>";
            if (tab[0] === "landings" && self.landings.length) badge = '<span class="badge">' + self.landings.length + "</span>";
            return '<span class="tab' + (self.tab === tab[0] ? " on" : "") + '" data-act="tab:' + tab[0] + '">' + tab[1] + badge + "</span>";
        }).join("");
    };

    // Now: the approach monitor during Approach/Landing, a compact readout otherwise.
    Board.prototype.nowHtml = function () {
        var live = this.recording() ? this.freshLive() : null;
        if (!live) return '<div class="note">' + this.idleText() + "</div>" + this.lastLandingHtml();
        var hot = !!APPROACH[live.phase], over = isNum(live.ias_kt) && isNum(live.vfe_kt) ? live.ias_kt - live.vfe_kt : null;
        var iasCls = over === null ? "" : over > 0 ? " bad" : over > -5 ? " near" : "";
        var html = '<div class="tiles' + (hot ? " hot" : "") + '">' +
            tile("main", hot ? "Height AAL" : "AAL", num(live.aal_ft, 0), "ft", "") +
            tile("main", "IAS", num(live.ias_kt, 0), isNum(live.vfe_kt) ? "kt · VFE " + num(live.vfe_kt, 0) : "kt", iasCls) +
            tile("main", "V/S", num(live.vs_fpm, 0), "fpm", "") +
            tile("aux", "Flaps", num(live.flap_deg, 0), "°", "") +
            tile("aux", "Bank", isNum(live.bank_deg) ? num(Math.abs(live.bank_deg), 0) : "–", "°", "") +
            "</div>";
        if (hot && live.gates && live.gates.length) {
            html += '<div class="gates">' + live.gates.map(function (g) { return gateHtml(g, live.aal_ft); }).join("") + "</div>";
        }
        if (!hot) html += this.recentHtml() + this.lastLandingHtml();
        return html;
    };

    Board.prototype.idleText = function () {
        if (this.stale()) return "The FOQA-Sim app is not connected. Start it on this PC; the panel finds it within a few seconds.";
        var s = this.status.status;
        if (s.state === "recording") return "Recording. Live values appear once the app sends them.";
        if (s.state === "connected") return "Ready. Recording starts when you load into a flight, or from Start new flight on the Flight tab.";
        if (s.state === "waiting_for_sim") return "The app is waiting for the sim.";
        return "Recorder error" + (s.message ? ": " + esc(s.message) : ".");
    };

    Board.prototype.recentHtml = function () {
        var ev = this.events ? this.events.events.slice(-TOP_EVENTS).reverse() : [];
        if (!ev.length) return "";
        return '<div class="section">Latest events</div>' + ev.map(this.eventHtml, this).join("");
    };

    Board.prototype.lastLandingHtml = function () {
        return this.landings.length ? '<div class="section">Last landing</div>' + landingRow(this.landings[0]) : "";
    };

    Board.prototype.eventHtml = function (e) {
        return '<div class="event lvl-' + e.level + '"><div class="erow"><span class="name">' + esc(e.title) + "</span>" +
            '<span class="time">' + clock(e.start_s) + "</span></div>" +
            (e.detail ? '<div class="detail">' + esc(e.detail) + "</div>" : "") + "</div>";
    };

    Board.prototype.eventsHtml = function () {
        var msg = this.events;
        if (!msg) return '<div class="note">No events heard yet. They appear here while a flight is recorded.</div>';
        var html = '<div class="counts">' + LEVELS.map(function (l) {
            var n = msg.counts[l] || 0;
            return '<span class="count lvl-' + l + (n ? "" : " zero") + '"><b>' + n + "</b> " + l + "</span>";
        }).join("") + "</div>";
        if (!msg.events.length) return html + '<div class="note">No events in this flight so far.</div>';
        return html + msg.events.slice().reverse().map(this.eventHtml, this).join("");
    };

    Board.prototype.landingsHtml = function () {
        if (!this.landings.length) return '<div class="note">No landings in this flight yet. After touchdown the landing opens here by itself.</div>';
        var html = landingCard(this.landings[0], !!this.backTab);
        if (this.landings.length > 1) html += '<div class="section">Earlier</div>' + this.landings.slice(1).map(landingRow).join("");
        return html;
    };

    Board.prototype.flightHtml = function () {
        var rec = this.recording(), f = rec && this.status.flight, html = "";
        if (rec) {
            var c = this.events ? this.events.counts : null, p = this.phase();
            var counts = c ? LEVELS.filter(function (l) { return c[l]; }).map(function (l) { return c[l] + " " + l; }).join(", ") || "none" : "–";
            html += '<div class="facts">' +
                fact("Aircraft", f && f.aircraft) + fact("From", f && f.departure) +
                fact("Time", clock(f ? f.elapsed_s : this.status.status.elapsed_s)) + fact("Phase", p && p.name) +
                fact("Landings", f ? String(f.landings) : null) + fact("Events", counts) +
                fact("Flight", this.status.status.flight_id) + "</div>";
        } else {
            html += '<div class="note">' + this.idleText() + "</div>";
        }
        var busy = this.pending ? this.pending.cmd : null, off = !rec || busy ? " off" : "";
        var confirming = this.now() - this.confirmAt <= CONFIRM_MS;
        html += '<div class="actions">';
        if (this.ready()) {
            html += '<span class="btn wide primary' + (busy ? " off" : "") + '" data-act="start">' +
                (busy === "start_flight" ? CMD_TEXT.start_flight.busy : "Start new flight") + "</span>";
        }
        html +=
            '<span class="btn wide' + off + '" data-act="mark">' + (busy === "mark" ? CMD_TEXT.mark.busy : "Mark event") + "</span>" +
            '<span class="btn wide' + off + '" data-act="end">' + (busy === "end_flight" ? CMD_TEXT.end_flight.busy : "End flight &amp; save") + "</span>";
        if (confirming && rec && !busy) {
            html += '<span class="btn wide danger armed" data-act="discard">Delete this flight</span>' +
                '<span class="btn wide" data-act="cancel">Keep it</span>';
        } else {
            html += '<span class="btn wide danger' + off + '" data-act="discard">' +
                (busy === "discard_flight" ? CMD_TEXT.discard_flight.busy : "Discard flight…") + "</span>";
        }
        html += "</div>";
        html += '<div class="hint">' + (confirming && rec
            ? "Discard deletes this recording for good. Click Delete again to confirm."
            : this.ready()
                ? "No flight is recording. Start records from here on, without leaving to the menu."
                : "Mark puts a pilot report at this moment. End saves the flight now, as leaving to the menu does.") + "</div>";
        return html;
    };

    function tile(kind, label, value, unit, cls) {
        return '<div class="tile ' + kind + cls + '"><span class="label">' + label + '</span><span class="value">' + value +
            '</span><span class="unit">' + esc(unit) + "</span></div>";
    }

    function gateHtml(g, aal) {
        var body;
        if (g.state === "ahead") body = isNum(aal) && aal > g.at_ft ? "in " + num(aal - g.at_ft, 0) + " ft" : "ahead";
        else if (g.state === "stable") body = "All checks met";
        else {
            body = g.checks.filter(function (c) { return !c.ok; }).map(function (c) {
                return '<div class="fail">' + esc(checkText(c)) + "</div>";
            }).join("") || "Unstable";
        }
        return '<div class="gate ' + g.state + '"><div class="ghead"><span class="at">' + g.at_ft + " ft</span>" +
            '<span class="gstate">' + g.state + '</span></div><div class="gbody">' + body + "</div></div>";
    }

    function fact(label, value) {
        return '<div class="fact"><span class="label">' + label + '</span><span class="fv">' + esc(value || "–") + "</span></div>";
    }

    function landingRow(v) {
        return '<div class="lrow lvl-' + (v.worst || "none") + '" data-act="tab:landings">' +
            '<span class="rw">' + esc(v.runway || "Off airport") + "</span>" +
            '<span class="nums">' + v.sink + " fpm · " + v.g + " g" + (v.along ? " · " + v.along : "") + "</span>" +
            (v.zone ? '<span class="tag ' + (v.zoneOk ? "ok" : "warn") + '">' + v.zone + "</span>" : "") + "</div>";
    }

    function landingCard(v, back) {
        var html = '<div class="card lvl-' + (v.worst || "none") + '">' +
            '<div class="head"><span class="title">Landing' + (v.runway ? " · " + esc(v.runway) : "") + "</span>" +
            (back ? '<span class="btn" data-act="back">×</span>' : "") + "</div>" +
            '<div class="stats">' +
            '<div class="stat"><span class="value">' + v.sink + '</span><span class="unit">fpm sink</span></div>' +
            '<div class="stat"><span class="value">' + v.g + '</span><span class="unit">g peak</span></div>' +
            (v.ias ? '<div class="stat"><span class="value">' + v.ias + '</span><span class="unit">kt IAS</span></div>' : "") +
            "</div>";
        var tags = "";
        if (v.zone) tags += '<span class="tag ' + (v.zoneOk ? "ok" : "warn") + '">' + v.zone + "</span>";
        if (v.bounce) tags += '<span class="tag warn">Bounce</span>';
        if (v.runway) tags += '<span class="where">' + v.along + " past threshold · " + v.cross + "</span>";
        if (tags) html += '<div class="tags">' + tags + "</div>";
        if (v.events.length) {
            html += v.events.map(function (e) {
                return '<div class="event lvl-' + e.level + '"><div class="erow"><span class="name">' + esc(e.title) + "</span></div>" +
                    (e.detail ? '<div class="detail">' + esc(e.detail) + "</div>" : "") + "</div>";
            }).join("");
            if (v.more) html += '<div class="note">+' + v.more + " more in the FOQA-Sim app</div>";
        } else {
            html += '<div class="clean">No events on this approach and landing</div>';
        }
        return html + "</div>";
    }

    Board.prototype.onAck = function (f) { this.ackHooks.push(f); };

    // Renders into `root` what `bus` hears and sends the pilot's commands on
    // it, with the attempt strip under it (foqa-strip.js, when loaded).
    // bus: { on(name, cb(json)), send(name, json) }.
    function attach(root, bus, now) {
        var board = new Board(root, bus, now);
        if (typeof FoqaStrip !== "undefined") {
            var strip = board.strip = FoqaStrip.attach(bus, now);
            strip.mount(root.querySelector(".strip-slot"));
            board.onAck(function (ack) { strip.onAck(ack); });
        }
        bus.on(MSG.status, function (json) { board.onStatus(json); });
        bus.on(MSG.live, function (json) { board.onLive(json); });
        bus.on(MSG.events, function (json) { board.onEvents(json); });
        bus.on(MSG.landing, function (json) { board.onLanding(json); });
        bus.on(MSG.landings, function (json) { board.onLandings(json); });
        return board;
    }

    return { MSG: MSG, attach: attach, Board: Board, landingView: landingView, holdRedraws: holdRedraws };
})();
