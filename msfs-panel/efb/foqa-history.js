// FOQA-Sim flight history: asks the FOQA-Sim app over the Comm Bus for the
// recent flights and one flight's compact debrief (docs/ingame-panel.md)
// and renders them into one element. Plain ES2015 and DOM only, like
// foqa-board.js, whose landingView it reuses (load it first).
// bus: { on(name, cb(json)), send(name, json) }.
var FoqaHistory = (function () {
    "use strict";

    var MSG = { flights: "FOQA_SIM_FLIGHTS", debrief: "FOQA_SIM_DEBRIEF", cmd: "FOQA_SIM_CMD" };
    var REPLY_WAIT_MS = 8000;   // the app repeats a reply for ~5 s
    var LEVELS = ["high", "medium", "low", "advisory"];
    var MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    var reqs = 0;

    function esc(s) {
        return String(s).replace(/[&<>"]/g, function (c) {
            return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c];
        });
    }

    function clock(s) {
        s = Math.max(0, Math.floor(s));
        var h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, sec = s % 60;
        return (h ? h + ":" + (m < 10 ? "0" : "") : "") + m + ":" + (sec < 10 ? "0" : "") + sec;
    }

    // "26 Oct 18:05Z"
    function when(utc) {
        var d = utc ? new Date(utc) : null;
        if (!d || isNaN(d.getTime())) return "Date unknown";
        var hh = d.getUTCHours(), mm = d.getUTCMinutes();
        return d.getUTCDate() + " " + MONTHS[d.getUTCMonth()] + " " + (hh < 10 ? "0" : "") + hh + ":" + (mm < 10 ? "0" : "") + mm + "Z";
    }

    function route(f) {
        return f.departure || f.arrival ? (f.departure || "?") + " → " + (f.arrival || "?") : "No airport";
    }

    function countsHtml(c) {
        var html = LEVELS.filter(function (l) { return c && c[l]; }).map(function (l) {
            return '<span class="count lvl-' + l + '"><b>' + c[l] + "</b> " + l + "</span>";
        }).join("");
        return html || '<span class="clean">No events</span>';
    }

    function History(root, bus, now) {
        this.root = root;
        this.bus = bus;
        this.now = now || function () { return Date.now(); };
        this.flights = null;     // last FOQA_SIM_FLIGHTS.flights
        this.debriefs = {};      // id -> reassembled debrief
        this.open = null;        // id of the flight shown, null: the list
        this.wait = null;        // { req, cmd, id, at, parts, total }: asked, no full reply yet
        this.error = null;
        this.expanded = null;    // index of the event showing why/advice
        this.html = null;
        root.classList.add("foqa");
        root.classList.add("history");
        var self = this;
        FoqaSim.holdRedraws(root, this);
        root.addEventListener("click", function (e) {
            var el = e.target;
            while (el && el !== root && !el.getAttribute("data-act")) el = el.parentNode;
            if (el && el !== root) self.act(el.getAttribute("data-act"));
        });
        setInterval(function () { self.render(); }, 500);
    }

    History.prototype.ask = function (cmd, id) {
        var req = "h" + (++reqs) + "-" + this.now().toString(36), msg = { v: 2, cmd: cmd, req: req };
        if (id) msg.id = id;
        this.wait = { req: req, cmd: cmd, id: id, at: this.now(), parts: [], total: 0 };
        this.error = null;
        if (this.bus.send) this.bus.send(MSG.cmd, JSON.stringify(msg));
        this.render();
    };

    // The list, fresh each time the Flights tab is shown.
    History.prototype.show = function () {
        if (!this.open && !this.wait) this.ask("list_flights");
    };

    History.prototype.onFlights = function (json) {
        var msg = JSON.parse(json);
        if (!this.wait || msg.req !== this.wait.req) return;   // a repeat, or another view's
        this.wait = null;
        if (msg.error) this.error = msg.error;
        else this.flights = msg.flights;
        this.render();
    };

    // Parts of one JSON document, joined by seq once all `total` are in.
    History.prototype.onDebrief = function (json) {
        var msg = JSON.parse(json), w = this.wait;
        if (!w || msg.req !== w.req) return;
        if (msg.error) { this.wait = null; this.error = msg.error; this.render(); return; }
        w.total = msg.total;
        w.parts[msg.seq] = msg.part;
        for (var i = 0; i < w.total; i++) if (typeof w.parts[i] !== "string") return;
        this.wait = null;
        try {
            this.debriefs[msg.id] = JSON.parse(w.parts.join(""));
        } catch (e) {
            this.error = "The debrief arrived damaged";
        }
        this.render();
    };

    History.prototype.act = function (act) {
        if (act.indexOf("flight:") === 0) {
            this.open = act.slice(7);
            this.expanded = null;
            this.error = null;
            if (!this.debriefs[this.open]) this.ask("debrief", this.open);
        } else if (act === "back") { this.open = null; this.wait = null; this.error = null; this.ask("list_flights"); }
        else if (act === "retry") this.ask(this.open ? "debrief" : "list_flights", this.open);
        else if (act.indexOf("ev:") === 0) { var i = +act.slice(3); this.expanded = this.expanded === i ? null : i; }
        else return;
        this.render();
    };

    History.prototype.render = function () {
        var w = this.wait;
        if (w && this.now() - w.at > REPLY_WAIT_MS) {
            this.wait = null;
            this.error = w.total ? "Incomplete reply (" + w.parts.filter(String).length + " of " + w.total + " parts)"
                : "No reply from the FOQA-Sim app. Is it running on this PC?";
        }
        if (this.pressing) return;
        var html = this.open ? this.debriefHtml() : this.listHtml();
        if (html === this.html) return;
        var body = this.root.querySelector(".body"), keep = body && this.root.getAttribute("data-view") === (this.open || "");
        var scroll = keep ? body.scrollTop : 0;
        this.html = html;
        this.root.setAttribute("data-view", this.open || "");
        this.root.innerHTML = html;
        this.root.querySelector(".body").scrollTop = scroll;
    };

    History.prototype.stateHtml = function (what) {
        if (this.wait) return '<div class="note">Asking the FOQA-Sim app for ' + what + "…</div>";
        if (this.error) return '<div class="note">' + esc(this.error) + '</div><span class="btn wide" data-act="retry">Try again</span>';
        return "";
    };

    History.prototype.listHtml = function () {
        var head = '<div class="top"><span class="state">Recent flights</span><span class="sub"></span>' +
            '<span class="btn" data-act="retry">Refresh</span></div>';
        var f = this.flights, body = "";
        if (f && f.length) {
            body = f.map(function (x) {
                return '<div class="frow lvl-' + (x.worst_level || "none") + '" data-act="flight:' + esc(x.id) + '">' +
                    '<div class="fline"><span class="fwhen">' + when(x.started_utc) + '</span><span class="fac">' + esc(x.aircraft || "") + "</span></div>" +
                    '<div class="fline"><span class="froute">' + esc(route(x)) + "</span>" +
                    '<span class="fnums">' + clock(x.duration_s) + " · " + x.landings + (x.landings === 1 ? " landing" : " landings") + "</span></div>" +
                    '<div class="counts">' + countsHtml(x.event_counts) + "</div></div>";
            }).join("");
        } else if (f) body = '<div class="note">No saved flights yet. A flight appears here once it is saved.</div>';
        return head + '<div class="body">' + this.stateHtml("your flights") + body + "</div>";
    };

    History.prototype.debriefHtml = function () {
        var d = this.debriefs[this.open], self = this;
        var head = '<div class="top"><span class="btn back" data-act="back">‹ Flights</span><span class="sub"></span></div>';
        if (!d) return head + '<div class="body">' + this.stateHtml("the debrief") + "</div>";
        var f = d.flight;
        var html = '<div class="dhead lvl-' + (f.worst_level || "none") + '"><div class="title">' + esc(route(f)) + "</div>" +
            '<div class="facts">' +
            fact("Date", when(f.started_utc)) + fact("Aircraft", f.aircraft) + fact("Time", clock(f.duration_s)) +
            fact("Landings", String(f.landings)) + fact("Profile", f.profile) + "</div>" +
            '<div class="counts">' + countsHtml(f.event_counts) + "</div></div>";
        if (d.touchdowns.length) {
            html += '<div class="section">Landings</div>' + d.touchdowns.map(function (td, i) {
                var v = FoqaSim.landingView({ flight_id: f.id, touchdown: td, events: [] });
                return '<div class="card"><div class="head"><span class="title">' + (i + 1) + " · " + esc(v.runway || "Off airport") +
                    '</span><span class="time">' + clock(td.t_s) + "</span></div>" +
                    '<div class="stats"><div class="stat"><span class="value">' + v.sink + '</span><span class="unit">fpm sink</span></div>' +
                    '<div class="stat"><span class="value">' + v.g + '</span><span class="unit">g peak</span></div>' +
                    (v.ias ? '<div class="stat"><span class="value">' + v.ias + '</span><span class="unit">kt IAS</span></div>' : "") + "</div>" +
                    '<div class="tags">' + (v.zone ? '<span class="tag ' + (v.zoneOk ? "ok" : "warn") + '">' + v.zone + "</span>" : "") +
                    (v.bounce ? '<span class="tag warn">Bounce</span>' : "") +
                    (v.runway ? '<span class="where">' + v.along + " past threshold · " + v.cross + "</span>" : "") + "</div></div>";
            }).join("");
        }
        html += '<div class="section">Events</div>';
        if (!d.events.length) html += '<div class="clean">No events in this flight</div>';
        html += d.events.map(function (e, i) {
            var detail = e.rule === "pilot-report" ? e.note : e.detail, open = self.expanded === i;
            var more = open ? (e.why_it_matters ? '<div class="why"><b>Why it matters</b> ' + esc(e.why_it_matters) + "</div>" : "") +
                (e.advice ? '<div class="why"><b>Advice</b> ' + esc(e.advice) + "</div>" : "") : "";
            return '<div class="event lvl-' + e.level + (open ? " open" : "") + '" data-act="ev:' + i + '"><div class="erow">' +
                '<span class="lvl">' + e.level + '</span><span class="name">' + esc(e.title) + '</span><span class="time">' + clock(e.start_s) + "</span></div>" +
                (detail ? '<div class="detail">' + esc(detail) + "</div>" : "") + more + "</div>";
        }).join("");
        return head + '<div class="body">' + html + "</div>";
    };

    function fact(label, value) {
        return '<div class="fact"><span class="label">' + label + '</span><span class="fv">' + esc(value || "–") + "</span></div>";
    }

    function attach(root, bus, now) {
        var h = new History(root, bus, now);
        bus.on(MSG.flights, function (json) { h.onFlights(json); });
        bus.on(MSG.debrief, function (json) { h.onDebrief(json); });
        h.render();
        return h;
    }

    return { MSG: MSG, attach: attach };
})();
