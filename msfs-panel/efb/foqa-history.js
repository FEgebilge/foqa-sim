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
    // Charts, in px: drawn at the pane's width (at least `minW`), so they
    // fill it and text keeps its size.
    var CHART = { minW: 320, panel: 84, gap: 22, axis: 22, left: 54, right: 8 };
    var COLORS = { line: "#58d1e5", grid: "#1f2c3f", text: "#8c9aa9", td: "#e8f0f6",
        high: "#f0605a", medium: "#f2a33a", low: "#e2cf5c", advisory: "#73aee6" };
    var TICK_STEPS = [30, 60, 120, 300, 600, 900, 1800, 3600, 7200];
    // The flight's assessment (debrief-api.md): label and the border colour class.
    var GRADES = { good: ["Good", "none"], fair: ["Fair", "medium"], poor: ["Poor", "high"] };
    var KEY_LEVELS = { high: true, medium: true };
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
        return f.departure || f.arrival ? (f.departure || "?") + " – " + (f.arrival || "?") : "No airport";
    }

    function grade(g) { return GRADES[g] || GRADES.good; }

    function gradeTag(g) { return '<span class="grade g-' + (GRADES[g] ? g : "good") + '">' + grade(g)[0] + "</span>"; }

    // Touchdowns as landings: each one off the air with the bounces after it.
    function landingsOf(tds) {
        var out = [];
        tds.forEach(function (td, i) {
            if (td.bounce && out.length) out[out.length - 1].bounces.push(td);
            else out.push({ td: td, index: i, bounces: [] });
        });
        return out;
    }

    // One row per rule, at its worst (earliest at equal level), most severe
    // first: high and medium ones and the pilot's own reports.
    function keyFindings(events) {
        var byRule = {};
        events.forEach(function (e, i) {
            var w = byRule[e.rule];
            if (!w) byRule[e.rule] = w = { i: i, n: 0 };
            else if (LEVELS.indexOf(e.level) < LEVELS.indexOf(events[w.i].level)) w.i = i;
            w.n++;
        });
        return Object.keys(byRule).map(function (r) { return byRule[r]; }).filter(function (w) {
            var e = events[w.i];
            return KEY_LEVELS[e.level] || e.rule === "pilot-report";
        }).sort(function (a, b) {
            var x = events[a.i], y = events[b.i];
            return LEVELS.indexOf(x.level) - LEVELS.indexOf(y.level) || x.start_s - y.start_s;
        });
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
        this.allEvents = false;  // every event, not just the key findings
        this.showParts = false;  // the assessment's breakdown
        this.showPhases = false; // the phase list
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
            this.allEvents = false;
            this.showParts = false;
            this.showPhases = false;
            this.error = null;
            if (!this.debriefs[this.open]) this.ask("debrief", this.open);
        } else if (act === "back") { this.open = null; this.wait = null; this.error = null; this.ask("list_flights"); }
        else if (act === "retry") this.ask(this.open ? "debrief" : "list_flights", this.open);
        else if (act.indexOf("ev:") === 0) { var i = +act.slice(3); this.expanded = this.expanded === i ? null : i; }
        else if (act === "parts") this.showParts = !this.showParts;
        else if (act === "phases") this.showPhases = !this.showPhases;
        else if (act === "all-events") this.allEvents = !this.allEvents;
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
                return '<div class="frow lvl-' + grade(x.grade)[1] + '" data-act="flight:' + esc(x.id) + '">' +
                    '<div class="fline"><span class="fwhen">' + when(x.started_utc) + '</span><span class="fac">' + esc(x.aircraft || "") + "</span>" +
                    gradeTag(x.grade) + "</div>" +
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
        var f = d.flight, events = d.events;
        var html = '<div class="dhead lvl-' + grade(f.grade)[1] + '"><div class="title">' + esc(route(f)) + "</div>" +
            '<div class="assess">' + scoreHtml(f) + gradeTag(f.grade) + '<span class="why">' + assessText(f) + "</span></div>" +
            barsHtml(f) +
            this.partsHtml(f) +
            '<div class="facts">' +
            fact("Date", when(f.started_utc)) + fact("Aircraft", f.aircraft) + fact("Time", clock(f.duration_s)) +
            fact("Landings", String(f.landings)) + fact("Profile", f.profile) + "</div></div>";
        // The body's padding (14 px a side) and scrollbar (10 px) off the pane.
        var width = this.root.clientWidth - 38, profile = profileChart(d, width);
        if (profile) html += '<div class="section">Flight profile</div>' + profile;
        html += this.phasesHtml(d.phases);
        var landings = landingsOf(d.touchdowns);
        if (landings.length) {
            html += '<div class="section">Landings</div>' + landings.map(function (l, n) {
                return landingCardHtml(d, l, n, width - 26);
            }).join("");
        }
        var key = keyFindings(events), shown = this.allEvents
            ? events.map(function (e, i) { return { i: i, n: 1 }; }) : key;
        html += '<div class="section">' + (this.allEvents ? "All events" : "Key findings") + "</div>";
        if (!shown.length) {
            html += '<div class="clean">' + (events.length ? "Nothing high or medium: " + events.length + " minor " +
                (events.length === 1 ? "event" : "events") : "No events in this flight") + "</div>";
        }
        html += shown.map(function (w) { return eventHtml(events[w.i], w.i, w.n, self.expanded === w.i); }).join("");
        if (events.length > key.length) {
            html += '<span class="btn wide" data-act="all-events">' +
                (this.allEvents ? "Key findings only" : "Show all " + events.length + " events") + "</span>";
        }
        return head + '<div class="body">' + html + "</div>";
    };

    // "Hard landing (high) 4 · Flap overspeed (medium) 2 = 6 → Poor", with the grades' reach, behind a tap.
    History.prototype.partsHtml = function (f) {
        if (!f.parts || !f.parts.length) return "";
        var open = this.showParts, html = '<div class="parts-toggle" data-act="parts">' + (open ? "Hide" : "How this was graded") + "</div>";
        if (!open) return html;
        var line = f.parts.map(function (p) {
            return esc(p.title) + " (" + p.level + ")" + (p.count > 1 ? " ×" + p.count : "") + " " + p.points;
        }).join(" · ");
        var bands = (f.bands || []).map(function (b, i) {
            var prev = i ? f.bands[i - 1].up_to : -1;
            return grade(b.grade)[0] + " " + (b.up_to === undefined ? prev + 1 + "+" : "≤" + b.up_to);
        }).join(" · ");
        return html + '<div class="parts">' + line + " = " + f.points + " → " + grade(f.grade)[0] +
            (bands ? '<div class="bands">' + bands + "</div>" : "") + "</div>";
    };

    // The phases the flight was split into, each with what opened it, behind a tap.
    History.prototype.phasesHtml = function (phases) {
        if (!phases || !phases.length) return "";
        var open = this.showPhases, html = '<div class="parts-toggle" data-act="phases">' + (open ? "Hide phases" : "Phases (" + phases.length + ")") + "</div>";
        if (!open) return html;
        return html + '<div class="phases">' + phases.map(function (p) {
            return '<div class="phrow"><span class="time">' + clock(p.start_s) + '</span><span class="name">' + esc(phaseName(p.phase)) +
                '</span><span class="trigger">' + esc(p.trigger) + "</span></div>";
        }).join("") + "</div>";
    };

    // "EnRoute" -> "En route"
    function phaseName(p) {
        return p.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/ (\w)/g, function (m, c) { return " " + c.toLowerCase(); });
    }

    // The landing-2 score (debrief-api.md), "77.4/100", before the grade.
    function scoreHtml(f) {
        if (f.score === undefined || f.score === null) return "";
        return '<span class="score">' + f.score.toFixed(1) + '<span class="of">/100</span></span>';
    }

    // The grading landing's six domains as mini-bars (track as long as the weight, the points lost shaded in the level's
    // colour) and the largest deduction in one line.
    var DOMAIN_SHORT = ["Approach", "Flare", "Point", "Lateral", "Contact", "Rollout"];
    var BAR_LEVEL = ["none", "low", "medium", "high"];
    function barsHtml(f) {
        if (!f.bars || !f.bars.length) return "";
        var html = '<div class="mbars">' + f.bars.map(function (b, i) {
            var weight = b[0], lost = b[1], level = BAR_LEVEL[Math.min(3, b[2])] || "none";
            var track = '<div class="mtrack' + (lost === null ? " na" : "") + '" style="width:' + (weight / 25 * 100).toFixed(0) + '%">' +
                (lost ? '<div class="mlost lvl-' + level + '" style="width:' + Math.min(100, lost / weight * 100).toFixed(0) + '%"></div>' : "") + "</div>";
            return '<div class="mbar"><div class="mname">' + DOMAIN_SHORT[i] + '<span class="mpts">' +
                (lost === null ? "n/a" : lost ? "−" + lost.toFixed(1) : "0") + "</span></div>" + track + "</div>";
        }).join("") + "</div>";
        return html + (f.top ? '<div class="mtop">' + esc(f.top) + "</div>" : "");
    }

    // "landing-2, partial data", or "12 points from 5 findings": what the grade is made of.
    function assessText(f) {
        if (f.method === "landing-2") return "landing-2" + (f.confidence === "partial" ? " · partial data, not rankable" : "");
        if (!f.findings) return "No findings at low or above";

        return f.points + (f.points === 1 ? " point" : " points") + " from " + f.findings +
            (f.findings === 1 ? " finding" : " findings") + " · each rule counts once, at its worst level";
    }

    function eventHtml(e, i, times, open) {
        var detail = e.rule === "pilot-report" ? e.note : e.detail;
        var more = open ? (e.why_it_matters ? '<div class="why"><b>Why it matters</b> ' + esc(e.why_it_matters) + "</div>" : "") +
            (e.advice ? '<div class="why"><b>Advice</b> ' + esc(e.advice) + "</div>" : "") : "";
        return '<div class="event lvl-' + e.level + (open ? " open" : "") + '" data-act="ev:' + i + '"><div class="erow">' +
            '<span class="lvl">' + e.level + '</span><span class="name">' + esc(e.title) + (times > 1 ? " ×" + times : "") +
            '</span><span class="time">' + clock(e.start_s) + "</span></div>" +
            (detail ? '<div class="detail">' + esc(detail) + "</div>" : "") + more + "</div>";
    }

    // One landing: its first contact graded, the bounces after it in a line.
    function landingCardHtml(d, l, n, width) {
        var td = l.td, v = FoqaSim.landingView({ flight_id: d.flight.id, touchdown: td, events: [] }), b = l.bounces;
        var bounced = b.length ? '<div class="bounces">Then ' + b.map(function (x) {
            return clock(x.t_s) + " · " + num(x.sink_fpm) + " fpm · " + num(x.peak_g, 2) + " g";
        }).join(", ") + "</div>" : "";
        return '<div class="card"><div class="head"><span class="title">' + (n + 1) + " · " + esc(v.runway || "Off airport") +
            '</span><span class="time">' + clock(td.t_s) + "</span></div>" +
            '<div class="stats"><div class="stat"><span class="value">' + v.sink + '</span><span class="unit">fpm sink</span></div>' +
            '<div class="stat"><span class="value">' + v.g + '</span><span class="unit">g peak</span></div>' +
            (v.ias ? '<div class="stat"><span class="value">' + v.ias + '</span><span class="unit">kt IAS</span></div>' : "") + "</div>" +
            '<div class="tags">' + (v.zone ? '<span class="tag ' + (v.zoneOk ? "ok" : "warn") + '">' + v.zone + "</span>" : "") +
            (b.length ? '<span class="tag warn">Bounced ×' + b.length + "</span>" : "") +
            (v.runway ? '<span class="where">' + v.along + " past threshold · " + v.cross + "</span>" : "") + "</div>" +
            bounced + approachChart(d, l.index, width) + "</div>";
    }

    function num(v, digits) { return typeof v === "number" && isFinite(v) ? v.toFixed(digits || 0) : "–"; }

    function svgText(x, y, text, anchor, color) {
        return '<text x="' + x + '" y="' + y + '" fill="' + (color || COLORS.text) + '" font-size="13" font-family="RobotoMono-Regular, monospace"' +
            (anchor ? ' text-anchor="' + anchor + '"' : "") + ">" + esc(text) + "</text>";
    }

    function vline(x, y0, y1, color, dash) {
        return '<line x1="' + x + '" y1="' + y0 + '" x2="' + x + '" y2="' + y1 + '" stroke="' + color + '" stroke-width="1.5"' +
            (dash ? ' stroke-dasharray="4 3"' : "") + "/>";
    }

    // Stacked panels on one time axis, from a grid (history.rs `Grid`).
    // spec: { grid, width, from, to, ticks: [[t, label]], marks: [{ t, color, dash }],
    //         panels: [{ key, label, zero }] }
    function chartSvg(spec) {
        var g = spec.grid, c = CHART, n = spec.panels.length, w = Math.max(c.minW, Math.floor(spec.width));
        var h = n * (c.panel + c.gap) + c.axis, plotW = w - c.left - c.right;
        var x = function (t) { return (c.left + (t - spec.from) / (spec.to - spec.from) * plotW).toFixed(1); };
        var out = "";
        spec.panels.forEach(function (p, k) {
            var top = k * (c.panel + c.gap) + c.gap, bottom = top + c.panel;
            var vals = (g.values && g.values[p.key]) || [], known = vals.filter(function (v) { return v !== null; });
            out += svgText(c.left, top - 6, p.label, null, COLORS.td);
            out += '<rect x="' + c.left + '" y="' + top + '" width="' + plotW + '" height="' + c.panel + '" fill="none" stroke="' + COLORS.grid + '"/>';
            if (!known.length) { out += svgText(c.left + plotW / 2, top + c.panel / 2 + 5, "No data", "middle"); return; }
            var lo = Math.min.apply(null, known), hi = Math.max.apply(null, known);
            if (p.zero) { lo = Math.min(lo, 0); hi = Math.max(hi, 0); }
            if (hi - lo < 1) { hi += 1; lo -= 1; }
            var y = function (v) { return (bottom - (v - lo) / (hi - lo) * c.panel).toFixed(1); };
            out += svgText(c.left - 6, top + 11, Math.round(hi), "end") + svgText(c.left - 6, bottom, Math.round(lo), "end");
            if (p.zero && lo < 0 && hi > 0) {
                out += '<line x1="' + c.left + '" y1="' + y(0) + '" x2="' + (c.left + plotW) + '" y2="' + y(0) + '" stroke="' + COLORS.grid + '"/>';
            }
            var d = "", pen = false;
            vals.forEach(function (v, i) {
                var t = g.start_s + i * g.step_s;
                if (v === null || t < spec.from || t > spec.to) { pen = false; return; }
                d += (pen ? "L" : "M") + x(t) + " " + y(v);
                pen = true;
            });
            out += '<path d="' + d + '" fill="none" stroke="' + COLORS.line + '" stroke-width="2"/>';
            spec.marks.forEach(function (m) {
                if (m.t >= spec.from && m.t <= spec.to) out += vline(x(m.t), top, bottom, m.color, m.dash);
            });
        });
        spec.ticks.forEach(function (tk) {
            if (tk[0] >= spec.from && tk[0] <= spec.to) out += svgText(x(tk[0]), h - 4, tk[1], "middle");
        });
        return '<svg class="chart" viewBox="0 0 ' + w + " " + h + '" width="' + w + '" height="' + h + '">' + out + "</svg>";
    }

    function eventMarks(events) {
        return events.map(function (e) { return { t: e.peak_s, color: COLORS[e.level] || COLORS.text }; });
    }

    // The last minutes to a touchdown: height, sink and speed against time to it.
    function approachChart(d, i, width) {
        var g = d.approaches && d.approaches[i], td = d.touchdowns[i].t_s;
        if (!g || !g.values) return "";
        var ticks = [-180, -120, -60, 0].map(function (s) { return [td + s, s ? "-" + clock(-s) : "TD"]; });
        return chartSvg({
            grid: g, width: width, from: td - 180, to: td + 10, ticks: ticks,
            marks: eventMarks(d.events).concat(d.touchdowns.map(function (x) { return { t: x.t_s, color: COLORS.td, dash: true }; })),
            panels: [{ key: "aal_ft", label: "Height AAL · ft" }, { key: "vs_fpm", label: "V/S · fpm", zero: true },
                { key: "ias_kt", label: "IAS · kt" }]
        });
    }

    // The whole flight: altitude and speed, landings and events marked.
    function profileChart(d, width) {
        var g = d.profile, vals = g && g.values && g.values.alt_ft;
        if (!vals || vals.length < 2) return "";
        var from = g.start_s, to = g.start_s + (vals.length - 1) * g.step_s;
        var step = TICK_STEPS.filter(function (s) { return (to - from) / s <= 6; })[0] || 7200, ticks = [];
        for (var t = Math.ceil(from / step) * step; t <= to; t += step) ticks.push([t, clock(t)]);
        var tds = d.touchdowns.map(function (td) { return { t: td.t_s, color: COLORS.td, dash: true }; });
        return chartSvg({
            grid: g, width: width, from: from, to: to, ticks: ticks, marks: eventMarks(d.events).concat(tds),
            panels: [{ key: "alt_ft", label: "Altitude · ft" }, { key: "ias_kt", label: "IAS · kt" }]
        });
    }

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
